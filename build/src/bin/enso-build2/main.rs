#![feature(explicit_generic_args_with_impl_trait)]
#![feature(once_cell)]
#![feature(exit_status_error)]
#![feature(associated_type_defaults)]
#![feature(is_some_with)]

pub mod arg;
pub use enso_build::prelude;

use enso_build::prelude::*;

use crate::arg::Cli;
use crate::arg::IsTargetSource;
use crate::arg::Target;
use anyhow::Context;
use clap::Parser;
use enso_build::args::BuildKind;
use enso_build::paths::generated::RepoRoot;
use enso_build::paths::TargetTriple;
use enso_build::project::gui;
use enso_build::project::gui::Gui;
use enso_build::project::ide;
use enso_build::project::ide::Ide;
use enso_build::project::project_manager;
use enso_build::project::project_manager::ProjectManager;
use enso_build::project::wasm;
use enso_build::project::wasm::Wasm;
use enso_build::project::IsTarget;
use enso_build::project::IsWatchable;
use enso_build::project::IsWatcher;
use enso_build::setup_octocrab;
use enso_build::source::CiRunSource;
use enso_build::source::ExternalSource;
use enso_build::source::GetTargetJob;
use enso_build::source::Source;
use futures_util::future::try_join;
use ide_ci::actions::workflow::is_in_env;
use ide_ci::global;
use ide_ci::models::config::RepoContext;
use ide_ci::programs::Git;
use std::any::type_name;
use std::any::TypeId;
use std::time::Duration;
use tokio::runtime::Runtime;
use tracing::level_filters::LevelFilter;
use tracing::span::Attributes;
use tracing::span::Record;
use tracing::subscriber::Interest;
use tracing::Event;
use tracing::Id;
use tracing::Metadata;
use tracing::Subscriber;
use tracing_subscriber::filter::Filtered;
use tracing_subscriber::layer::Filter;
use tracing_subscriber::layer::Layered;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;
use tracing_subscriber::Registry;

/// The basic, common information available in this application.
#[derive(Clone, Debug)]
pub struct BuildContext {
    /// GitHub API client.
    ///
    /// If authorized, it will count API rate limits against our identity and allow operations like
    /// managing releases.
    pub octocrab: Octocrab,

    /// Version to be built.
    ///
    /// Note that this affects only targets that are being built. If project parts are provided by
    /// other means, their version might be different.
    pub triple: TargetTriple,

    /// Directory being an `enso` repository's working copy.
    ///
    /// The directory is not required to be a git repository. It is allowed to use source tarballs
    /// as well.
    pub source_root: PathBuf,

    /// Remote repository is used for release-related operations. This also includes deducing a new
    /// version number.
    pub remote_repo: RepoContext,
}

impl BuildContext {
    pub async fn new(cli: &Cli) -> Result<Self> {
        let octocrab = setup_octocrab()?;
        let versions = enso_build::version::deduce_versions(
            &octocrab,
            BuildKind::Dev,
            Ok(&cli.repo_remote),
            &cli.repo_path,
        )
        .await?;
        let triple = TargetTriple::new(versions);
        triple.versions.publish()?;
        Ok(Self {
            octocrab,
            triple,
            source_root: cli.repo_path.clone(),
            remote_repo: cli.repo_remote.clone(),
        })
    }

    pub fn resolve<T: IsTargetSource + IsTarget>(
        &self,
        source: arg::Source<T>,
    ) -> Result<GetTargetJob<T>>
    where
        T: Resolvable,
    {
        let destination = source.output_path.output_path;
        let source = match source.source {
            arg::SourceKind::Build => Source::BuildLocally(T::resolve(self, source.build_args)?),
            arg::SourceKind::Local => Source::External(ExternalSource::LocalFile(
                source.path.clone().context("Missing path to the local artifacts!")?,
            )),
            arg::SourceKind::CiRun => Source::External(ExternalSource::CiRun(CiRunSource {
                octocrab:      self.octocrab.clone(),
                run_id:        source.run_id.context(format!(
                    "Missing run ID, please provide {} argument.",
                    T::RUN_ID_NAME
                ))?,
                repository:    self.remote_repo.clone(),
                artifact_name: source.artifact_name.clone(),
            })),
        };
        Ok(GetTargetJob { source, destination })
    }


    pub fn commit(&self) -> BoxFuture<'static, Result<String>> {
        let root = self.source_root.clone();
        async move {
            match ide_ci::actions::env::Sha.fetch() {
                Ok(commit) => Ok(commit),
                Err(_e) => Git::new(root).head_hash().await,
            }
        }
        .boxed()
    }

    pub fn js_build_info(&self) -> BoxFuture<'static, Result<gui::BuildInfo>> {
        let triple = self.triple.clone();
        let commit = self.commit();
        async move {
            Ok(gui::BuildInfo {
                commit:         commit.await?,
                name:           "Enso IDE".into(),
                version:        triple.versions.version.clone(),
                engine_version: triple.versions.version.clone(),
            })
        }
        .boxed()
    }

    pub fn pm_info(&self) -> enso_build::project::project_manager::BuildInput {
        enso_build::project::project_manager::BuildInput {
            octocrab:  self.octocrab.clone(),
            versions:  self.triple.versions.clone(),
            repo_root: self.source_root.clone(),
        }
    }

    pub fn resolve_inputs<T: Resolvable>(
        &self,
        inputs: <T as IsTargetSource>::BuildInput,
    ) -> Result<<T as IsTarget>::BuildInput> {
        T::resolve(self, inputs)
    }

    pub fn get<Target>(
        &self,
        target: Target,
        target_source: arg::Source<Target>,
    ) -> BoxFuture<'static, Result<Target::Artifact>>
    where
        Target: IsTarget + IsTargetSource + Send + Sync + 'static,
        Target: Resolvable,
    {
        let get_task = self.resolve(target_source);

        async move {
            info!("Getting target {}.", type_name::<Target>());
            let get_task = get_task?;

            // We upload only built artifacts. There would be no point in uploading something that
            // we've just downloaded.
            let should_upload_artifact =
                matches!(get_task.source, Source::BuildLocally(_)) && is_in_env();
            let artifact = get_target(&target, get_task).await?;
            info!(
                "Got target {}, should it be uploaded? {}",
                type_name::<Target>(),
                should_upload_artifact
            );
            if should_upload_artifact {
                let upload_job = target.upload_artifact(ready(Ok(artifact.clone())));
                // global::spawn(upload_job);
                // info!("Spawned upload job for {}.", type_name::<Target>());
                warn!("Forcing the job.");
                upload_job.await?;
            }
            Ok(artifact)
        }
        .boxed()
    }

    pub fn repo_root(&self) -> RepoRoot {
        RepoRoot::new(&self.source_root, &self.triple.to_string())
    }

    pub fn handle_wasm(&self, wasm: arg::wasm::Target) -> BoxFuture<'static, Result> {
        match wasm.command {
            arg::wasm::Command::Watch { params, output_path } => {
                let inputs = self.resolve_inputs::<Wasm>(params);
                async move {
                    let mut watcher = Wasm.setup_watcher(inputs?, output_path.output_path).await?;
                    watcher.wait_ok().await
                }
                .boxed()
            }
            arg::wasm::Command::Build { params, output_path } => {
                let inputs = self.resolve_inputs::<Wasm>(params);
                async move { Wasm.build(inputs?, output_path.output_path).void_ok().await }.boxed()
            }
            arg::wasm::Command::Check => Wasm.check().boxed(),
            arg::wasm::Command::Test { no_wasm, no_native } =>
                Wasm.test(self.repo_root().path, !no_wasm, !no_native).boxed(),
            arg::wasm::Command::Get { source } => {
                let source = self.resolve(source);
                async move {
                    get_target(&Wasm, source?).await?;
                    Ok(())
                }
                .boxed()
            }
        }
    }

    pub fn handle_gui(&self, gui: arg::gui::Target) -> BoxFuture<'static, Result> {
        match gui.command {
            arg::gui::Command::Get { source } => {
                let job = self.get(Gui, source);
                job.void_ok().boxed()
            }
            arg::gui::Command::Watch { input } => self.watch_gui(input),
        }
    }

    pub fn watch_gui(&self, input: arg::gui::WatchInput) -> BoxFuture<'static, Result> {
        let arg::gui::WatchInput { wasm, output_path } = input;
        let wasm_watcher = self.resolve(wasm).map(|job| Wasm.watch(job));
        let repo_root = self.repo_root();
        let build_info = self.js_build_info();
        async move {
            let mut wasm_watcher = wasm_watcher?.await?;
            let input = gui::GuiInputs {
                repo_root,
                build_info,
                wasm: ready(Ok(wasm_watcher.as_ref().clone())).boxed(),
            };
            let mut gui_watcher = Gui.setup_watcher(input, output_path).await?;
            try_join(wasm_watcher.wait_ok(), gui_watcher.wait_ok()).void_ok().await
        }
        .boxed()
    }

    pub fn handle_project_manager(
        &self,
        project_manager: arg::project_manager::Target,
    ) -> BoxFuture<'static, Result> {
        let job = self.get(ProjectManager, project_manager.source);
        job.void_ok().boxed()
    }

    pub fn handle_ide(&self, ide: arg::ide::Target) -> BoxFuture<'static, Result> {
        match ide.command {
            arg::ide::Command::Build { params } => {
                let build_job = self.build_ide(params);
                async move {
                    let artifacts = build_job.await?;
                    if is_in_env() {
                        artifacts.upload().await?;
                    }
                    Ok(())
                }
                .boxed()
            }
            arg::ide::Command::Start { params } => {
                let build_job = self.build_ide(params);
                async move {
                    let ide = build_job.await?;
                    Command::new(ide.unpacked.join("Enso")).run_ok().await?;
                    Ok(())
                }
                .boxed()
            }
            arg::ide::Command::Watch { project_manager, gui } => {
                let gui_watcher = self.watch_gui(gui);
                let get_project_manager = self.get(ProjectManager, project_manager);
                let project_manager = async move {
                    let project_manager = get_project_manager.await?;
                    let p: &Path = project_manager.path.bin.project_managerexe.as_ref();
                    Ok(Command::new(p).run_ok())
                };
                try_join(gui_watcher, project_manager).void_ok().boxed()
            }
        }
    }

    pub fn build_ide(
        &self,
        params: arg::ide::BuildInput,
    ) -> BoxFuture<'static, Result<ide::Artifact>> {
        let arg::ide::BuildInput { gui, project_manager, output_path } = params;
        let input = ide::BuildInput {
            gui:             self.get(Gui, gui),
            project_manager: self.get(ProjectManager, project_manager),
            repo_root:       self.repo_root(),
            version:         self.triple.versions.version.clone(),
        };
        Ide.build(input, output_path)
    }
    //
    // pub async fn aaa(&self, source: arg::Source<Wasm>) -> Result {
    //     let wasm = self.resolve(source).context("Failed to resolve wasm description.")?;
    //     let (wasm_artifact, wasm_watcher) = match wasm.source {
    //         Source::BuildLocally(wasm_input) => {
    //             let watcher = Wasm.watch(wasm_input, wasm.destination.clone());
    //             let artifact = wasm::Artifact::from_existing(&wasm.destination);
    //             (artifact, watcher.ok())
    //         }
    //         external_source => (
    //             get_target(&Wasm, GetTargetJob {
    //                 destination: wasm.destination,
    //                 source:      external_source,
    //             }),
    //             None,
    //         ),
    //     };
    //
    //     let input = gui::GuiInputs {
    //         repo_root:  self.repo_root(),
    //         build_info: self.js_build_info(),
    //         wasm:       wasm_artifact,
    //     };
    //
    //     let gui_watcher = Gui.watch(input);
    //     if let Some(mut wasm_watcher) = wasm_watcher {
    //         try_join(wasm_watcher.watch_process.wait().anyhow_err(), gui_watcher).await?;
    //     } else {
    //         gui_watcher.await?;
    //     }
    //     Ok(())
    // }

    // pub fn gui_inputs(&self, wasm: arg::TargetSource<Wasm>) -> GuiInputs {
    //     let wasm = self.handle_wasm(WasmTarget { wasm });
    //     let build_info = self.js_build_info().boxed();
    //     let repo_root = self.repo_root();
    //     GuiInputs { wasm, build_info, repo_root }
    // }
    //
    // pub fn get_gui(&self, source: GuiTarget) -> BoxFuture<'static, Result> {
    //     match source.command {
    //         GuiCommand::Build => {
    //             let inputs = self.gui_inputs(source.wasm.clone());
    //             let build = self.get(Gui, source.gui, inputs);
    //             build.map(|_| Ok(())).boxed()
    //         }
    //         GuiCommand::Watch => {
    //             let build_info = self.js_build_info().boxed();
    //             let repo_root = self.repo_root();
    //             let watcher = Wasm.watch(self.wasm_build_input(), source.wasm.output_path);
    //             async move {
    //                 let wasm::Watcher { mut watch_process, artifacts } = watcher?;
    //                 let inputs =
    //                     GuiInputs { wasm: ready(Ok(artifacts)).boxed(), build_info, repo_root };
    //                 let js_watch = Gui.watch(inputs);
    //                 try_join(watch_process.wait().map_err(anyhow::Error::from), js_watch)
    //                     .map_ok(|_| ())
    //                     .await
    //             }
    //             .boxed()
    //         }
    //     }
    // }
}

pub trait Resolvable: IsTarget + IsTargetSource {
    fn resolve(
        ctx: &BuildContext,
        from: <Self as IsTargetSource>::BuildInput,
    ) -> Result<<Self as IsTarget>::BuildInput>;
}

impl Resolvable for Wasm {
    fn resolve(
        ctx: &BuildContext,
        from: <Self as IsTargetSource>::BuildInput,
    ) -> Result<<Self as IsTarget>::BuildInput> {
        Ok(wasm::BuildInput {
            repo_root:           ctx.repo_root(),
            crate_path:          from.crate_path,
            extra_cargo_options: from.cargo_options,
        })
    }
}

impl Resolvable for Gui {
    fn resolve(
        ctx: &BuildContext,
        from: <Self as IsTargetSource>::BuildInput,
    ) -> Result<<Self as IsTarget>::BuildInput> {
        Ok(gui::GuiInputs {
            wasm:       ctx.get(Wasm, from.wasm),
            repo_root:  ctx.repo_root(),
            build_info: ctx.js_build_info(),
        })
    }
}

impl Resolvable for ProjectManager {
    fn resolve(
        ctx: &BuildContext,
        _from: <Self as IsTargetSource>::BuildInput,
    ) -> Result<<Self as IsTarget>::BuildInput> {
        Ok(project_manager::BuildInput {
            repo_root: ctx.repo_root().path,
            octocrab:  ctx.octocrab.clone(),
            versions:  ctx.triple.versions.clone(),
        })
    }
}

pub fn get_target<Target: IsTarget>(
    target: &Target,
    job: GetTargetJob<Target>,
) -> BoxFuture<'static, Result<Target::Artifact>> {
    match job.source {
        Source::BuildLocally(inputs) => target.build(inputs, job.destination),
        Source::External(external) => target.get_external(external, job.destination),
    }
}

pub struct MyLayer;

impl<S: Subscriber + Debug + for<'a> LookupSpan<'a>> tracing_subscriber::Layer<S> for MyLayer {
    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        if metadata.module_path().is_some_with(|path| {
            path.starts_with("ide_ci::")
                || path.starts_with("enso_build")
                || path.starts_with("enso_build2")
        }) {
            Interest::always()
        } else {
            // dbg!(metadata);
            Interest::never()
        }
    }

    fn on_enter(&self, id: &Id, ctx: tracing_subscriber::layer::Context<'_, S>) {
        // ide_ci::global::println(format!("Enter {id:?}"));
    }
    fn on_exit(&self, id: &Id, ctx: tracing_subscriber::layer::Context<'_, S>) {
        // ide_ci::global::println(format!("Leave {id:?}"));
    }
    fn on_event(&self, event: &Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        // tracing_log::dbg!(event);
    }
    fn on_new_span(
        &self,
        attrs: &Attributes<'_>,
        id: &Id,
        ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let span = ctx.span(id).unwrap();
        let bar = global::new_spinner(format!("In span {id:?}: {:?}", span.name()));
        span.extensions_mut().insert(bar);
        ide_ci::global::println(format!("Create {id:?}"));
    }

    fn on_close(&self, id: Id, ctx: tracing_subscriber::layer::Context<'_, S>) {
        ide_ci::global::println(format!("Close {id:?}"));
    }
}


async fn main_internal() -> Result {
    let cli = Cli::parse();
    // console_subscriber::init();
    tracing::subscriber::set_global_default(
        Registry::default().with(MyLayer).with(tracing_subscriber::fmt::layer().without_time()),
    )
    .expect("default global");

    pretty_env_logger::init();
    debug!("Parsed CLI arguments: {cli:#?}");

    let ctx = BuildContext::new(&cli).instrument(info_span!("Building context.")).await?;
    match cli.target {
        Target::Wasm(wasm) => ctx.handle_wasm(wasm).await?,
        Target::Gui(gui) => ctx.handle_gui(gui).await?,
        Target::ProjectManager(project_manager) =>
            ctx.handle_project_manager(project_manager).await?,
        Target::Ide(ide) => ctx.handle_ide(ide).await?,
        // TODO: consider if out-of-source ./dist should be removed
        Target::Clean => Git::new(ctx.repo_root()).cmd()?.nice_clean().run_ok().await?,
    };
    info!("Completed main job.");
    global::complete_tasks().await?;
    Ok(())
}

fn main() -> Result {
    let rt = Runtime::new()?;
    rt.block_on(async { main_internal().await })?;
    rt.shutdown_timeout(Duration::from_secs(60 * 30));
    Ok(())
}
