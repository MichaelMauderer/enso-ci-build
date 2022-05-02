// #![feature(explicit_generic_args_with_impl_trait)]
// #![feature(once_cell)]
// #![feature(exit_status_error)]
// #![feature(associated_type_defaults)]
// #![feature(is_some_with)]
// #![feature(default_free_fn)]
// #![feature(adt_const_params)]

pub use crate::prelude;

use crate::prelude::*;

use crate::args::BuildKind;
use crate::cli::arg;
use crate::cli::arg::Cli;
use crate::cli::arg::IsTargetSource;
use crate::cli::arg::Target;
use crate::paths::generated::RepoRoot;
use crate::paths::TargetTriple;
use crate::project::gui;
use crate::project::gui::Gui;
use crate::project::ide;
use crate::project::ide::Ide;
use crate::project::project_manager;
use crate::project::project_manager::ProjectManager;
use crate::project::wasm;
use crate::project::wasm::Wasm;
use crate::project::IsTarget;
use crate::project::IsWatchable;
use crate::project::IsWatcher;
use crate::setup_octocrab;
use crate::source::CiRunSource;
use crate::source::ExternalSource;
use crate::source::GetTargetJob;
use crate::source::ReleaseSource;
use crate::source::Source;
use anyhow::Context;
use clap::Parser;
use futures_util::future::try_join;
use ide_ci::actions::workflow::is_in_env;
use ide_ci::cache::Cache;
use ide_ci::global;
use ide_ci::log::setup_logging;
use ide_ci::models::config::RepoContext;
use ide_ci::programs::cargo;
use ide_ci::programs::Cargo;
use ide_ci::programs::Git;
use std::any::type_name;
use std::time::Duration;
use tokio::process::Child;
use tokio::runtime::Runtime;


/// The basic, common information available in this application.
#[derive(Clone, Debug)]
pub struct BuildContext {
    /// GitHub API client.
    ///
    /// If authorized, it will count API rate limits against our identity and allow operations like
    /// managing releases or downloading CI run artifacts.
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

    pub cache: Cache,
}

impl BuildContext {
    pub async fn new(cli: &Cli) -> Result<Self> {
        let octocrab = setup_octocrab()?;
        let versions = crate::version::deduce_versions(
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
            cache: Cache::new(&cli.cache_path).await?,
        })
    }

    pub fn resolve<T: IsTargetSource + IsTarget>(
        &self,
        target: T,
        source: arg::Source<T>,
    ) -> BoxFuture<'static, Result<GetTargetJob<T>>>
    where
        T: Resolvable,
    {
        let span = info_span!("Resolving.", ?target, ?source).entered();
        let destination = source.output_path.output_path;
        let source = match source.source {
            arg::SourceKind::Build => {
                let resolved = T::resolve(self, source.build_args);
                ready(resolved.map(Source::BuildLocally)).boxed()
            }
            arg::SourceKind::Local => {
                let resolved = source.path.clone().context("Missing path to the local artifacts!");
                ready(resolved.map(|p| Source::External(ExternalSource::LocalFile(p)))).boxed()
            }
            arg::SourceKind::CiRun => {
                let run_id = source.run_id.context(format!(
                    "Missing run ID, please provide {} argument.",
                    T::RUN_ID_NAME
                ));
                ready(run_id.map(|run_id| {
                    Source::External(ExternalSource::CiRun(CiRunSource {
                        octocrab: self.octocrab.clone(),
                        run_id,
                        repository: self.remote_repo.clone(),
                        artifact_name: source.artifact_name.clone(),
                    }))
                }))
                .boxed()
            }
            arg::SourceKind::Release => {
                let designator = source
                    .release
                    .context(format!("Missing {} argument.", T::RELEASE_DESIGNATOR_NAME));
                let resolved = designator
                    .map(|designator| self.resolve_release_designator(target, designator));
                async move { Ok(Source::External(ExternalSource::Release(resolved?.await?))) }
                    .boxed()
            }
        };
        async move { Ok(GetTargetJob { source: source.await?, destination }) }
            .instrument(span.clone())
            .boxed()
    }

    #[tracing::instrument]
    pub fn resolve_release_designator<T: IsTarget>(
        &self,
        target: T,
        designator: String,
    ) -> BoxFuture<'static, Result<ReleaseSource>> {
        let repository = self.remote_repo.clone();
        let octocrab = self.octocrab.clone();
        async move {
            let release = match designator.as_str() {
                "latest" => repository.latest_release(&octocrab).await?,
                "nightly" => crate::version::latest_nightly_release(&octocrab, &repository).await?,
                tag => repository.find_release_by_text(&octocrab, tag).await?,
            };
            Ok(ReleaseSource {
                octocrab,
                repository,
                asset_id: target.find_asset(release.assets)?.id,
            })
        }
        .boxed()
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

    pub fn pm_info(&self) -> crate::project::project_manager::BuildInput {
        crate::project::project_manager::BuildInput {
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
        let get_task = self.resolve(target.clone(), target_source);
        let cache = self.cache.clone();
        async move {
            info!("Getting target {}.", type_name::<Target>());
            let get_task = get_task.await?;

            // We upload only built artifacts. There would be no point in uploading something that
            // we've just downloaded.
            let should_upload_artifact =
                matches!(get_task.source, Source::BuildLocally(_)) && is_in_env();
            let artifact = target.get(get_task, cache).await?;
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
                async move { Wasm.build_locally(inputs?, output_path.output_path).void_ok().await }
                    .boxed()
            }
            arg::wasm::Command::Check => Wasm.check().boxed(),
            arg::wasm::Command::Test { no_wasm, no_native } =>
                Wasm.test(self.repo_root().path, !no_wasm, !no_native).boxed(),
            arg::wasm::Command::Get { source } => {
                let target = Wasm;
                let source = self.resolve(target, source);
                let cache = self.cache.clone();
                async move {
                    target.get(source.await?, cache).await?;
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
        let wasm_target = Wasm;
        let arg::gui::WatchInput { wasm, output_path } = input;
        let source = self.resolve(wasm_target, wasm);
        let repo_root = self.repo_root();
        let build_info = self.js_build_info();
        let cache = self.cache.clone();
        async move {
            let source = source.await?;
            let mut wasm_watcher = wasm_target.watch(source, cache).await?;
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
                use crate::project::ProcessWrapper;
                let gui_watcher = self.watch_gui(gui);
                let project_manager = self.spawn_project_manager(project_manager);
                let wait_for_pm_to_finish = async move { project_manager.await?.wait_ok().await };
                // TODO record result
                try_join(gui_watcher, wait_for_pm_to_finish).void_ok().boxed()
            }
            arg::ide::Command::IntegrationTest { external_backend, project_manager } => {
                let project_manager = self.spawn_project_manager(project_manager);
                let source_root = self.source_root.clone();
                async move {
                    let project_manager =
                        if !external_backend { Some(project_manager.await?) } else { None };
                    Wasm.integration_test(source_root, project_manager).await
                }
                .boxed()
            }
        }
    }

    /// Spawns a Project Manager.
    pub fn spawn_project_manager(
        &self,
        source: arg::Source<ProjectManager>,
    ) -> BoxFuture<'static, Result<Child>> {
        let get_task = self.get(ProjectManager, source);
        async move {
            let project_manager = get_task.await?;
            let p: &Path = project_manager.path.bin.project_managerexe.as_ref();
            Command::new(p).spawn_intercepting()
        }
        .boxed()
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
        let arg::wasm::BuildInputs { crate_path, wasm_profile, cargo_options, profiling_level } =
            from;
        Ok(wasm::BuildInput {
            repo_root: ctx.repo_root(),
            crate_path,
            extra_cargo_options: cargo_options,
            profile: wasm_profile.into(),
            profiling_level: profiling_level.map(into),
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

pub async fn main_internal() -> Result {
    let cli = Cli::parse();
    setup_logging()?;

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
        Target::Lint => {
            Cargo
                .cmd()?
                .current_dir(ctx.repo_root())
                .arg("clippy")
                .apply(&cargo::Options::Workspace)
                .apply(&cargo::Options::Package("enso-integration-test".into()))
                .apply(&cargo::Options::AllTargets)
                .args(["--", "-D", "warnings"])
                .run_ok()
                .await?;

            Cargo
                .cmd()?
                .current_dir(ctx.repo_root())
                .arg("fmt")
                .args(["--", "--check"])
                .run_ok()
                .await?;
        }
    };
    info!("Completed main job.");
    global::complete_tasks().await?;
    Ok(())
}

pub fn main() -> Result {
    let rt = Runtime::new()?;
    rt.block_on(async { main_internal().await })?;
    rt.shutdown_timeout(Duration::from_secs(60 * 30));
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::Versions;

    #[tokio::test]
    async fn resolving_release() -> Result {
        setup_logging()?;
        let octocrab = Octocrab::default();
        let context = BuildContext {
            remote_repo: RepoContext::from_str("enso-org/enso")?,
            triple: TargetTriple::new(Versions::new(Version::new(2022, 1, 1))),
            source_root: r"H:/NBO/enso5".into(),
            octocrab,
            cache: Cache::new_default().await?,
        };

        dbg!(context.resolve_release_designator(ProjectManager, "latest".into()).await)?;

        Ok(())
    }
}