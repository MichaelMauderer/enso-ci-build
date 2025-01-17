use crate::prelude::*;

use crate::env::new::RawVariable;
use heck::ToKebabCase;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

pub fn wrap_expression(expression: impl AsRef<str>) -> String {
    format!("${{{{ {} }}}}", expression.as_ref())
}

pub fn env_expression(environment_variable: &impl RawVariable) -> String {
    wrap_expression(format!("env.{}", environment_variable.name()))
}


pub fn is_github_hosted() -> String {
    "startsWith(runner.name, 'GitHub Actions') || startsWith(runner.name, 'Hosted Agent')".into()
}

pub fn setup_conda() -> Step {
    // use crate::actions::workflow::definition::step::CondaChannel;
    Step {
        name: Some("Setup conda (GH runners only)".into()),
        uses: Some("s-weigand/setup-conda@v1.0.5".into()),
        r#if: Some(is_github_hosted()),
        with: Some(step::Argument::SetupConda {
            update_conda:   Some(false),
            conda_channels: Some("anaconda, conda-forge".into()),
        }),
        ..default()
    }
}

pub fn setup_wasm_pack_step() -> Step {
    Step {
        name: Some("Installing wasm-pack".into()),
        uses: Some("jetli/wasm-pack-action@v0.3.0".into()),
        with: Some(step::Argument::Other(BTreeMap::from_iter([(
            "version".into(),
            "v0.10.2".into(),
        )]))),
        r#if: Some(is_github_hosted()),
        ..default()
    }
}

pub fn setup_artifact_api() -> Step {
    let script = [
        r#"core.exportVariable("ACTIONS_RUNTIME_TOKEN", process.env["ACTIONS_RUNTIME_TOKEN"])"#,
        r#"core.exportVariable("ACTIONS_RUNTIME_URL", process.env["ACTIONS_RUNTIME_URL"])"#,
        r#"core.exportVariable("GITHUB_RETENTION_DAYS", process.env["GITHUB_RETENTION_DAYS"])"#,
    ]
    .join("\n");
    Step {
        name: Some("Setup the Artifact API environment".into()),
        uses: Some("actions/github-script@v6".into()),
        with: Some(step::Argument::GitHubScript { script }),
        ..default()
    }
}

pub fn shell_os(os: OS, command_line: impl Into<String>) -> Step {
    Step {
        run: Some(command_line.into()),
        env: once(github_token_env()).collect(),
        r#if: Some(format!("runner.os {} 'Windows'", if os == OS::Windows { "==" } else { "!=" })),
        shell: Some(if os == OS::Windows { Shell::Pwsh } else { Shell::Bash }),
        ..default()
    }
}

pub fn shell(command_line: impl Into<String>) -> Step {
    Step { run: Some(command_line.into()), env: once(github_token_env()).collect(), ..default() }
}

pub fn run(run_args: impl AsRef<str>) -> Step {
    shell(format!("./run {}", run_args.as_ref()))
}

pub fn cancel_workflow_action() -> Step {
    Step {
        name: Some("Cancel Previous Runs".into()),
        uses: Some("styfle/cancel-workflow-action@0.9.1".into()),
        with: Some(step::Argument::Other(BTreeMap::from_iter([(
            "access_token".into(),
            "${{ github.token }}".into(),
        )]))),
        ..default()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobId(String);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", untagged)]
pub enum Concurrency {
    Plain(String),
    Map { group: String, cancel_in_progress: bool },
}

impl Concurrency {
    pub fn new(group_name: impl Into<String>) -> Self {
        Self::Plain(group_name.into())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Workflow {
    pub name:        String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub on:          Event,
    pub jobs:        BTreeMap<String, Job>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub env:         BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<Concurrency>,
}

impl Workflow {
    pub fn expose_outputs(&self, source_job_id: impl AsRef<str>, consumer_job: &mut Job) {
        let source_job = self.jobs.get(source_job_id.as_ref()).unwrap();
        consumer_job.use_job_outputs(source_job_id.as_ref(), source_job);
    }
}

impl Workflow {
    pub fn add_job(&mut self, job: Job) -> String {
        let key = job.name.to_kebab_case();
        self.jobs.insert(key.clone(), job);
        key
    }

    pub fn add<J: JobArchetype>(&mut self, os: OS) -> String {
        self.add_customized::<J>(os, |_| {})
    }

    pub fn add_customized<J: JobArchetype>(&mut self, os: OS, f: impl FnOnce(&mut Job)) -> String {
        let (key, mut job) = J::entry(os);
        f(&mut job);
        self.jobs.insert(key.clone(), job);
        key
    }

    pub fn add_dependent<J: JobArchetype>(
        &mut self,
        os: OS,
        needed: impl IntoIterator<Item: AsRef<str>>,
    ) -> String {
        let (key, mut job) = J::entry(os);
        for needed in needed {
            self.expose_outputs(needed.as_ref(), &mut job);
        }
        self.jobs.insert(key.clone(), job);
        key
    }

    pub fn env(&mut self, var_name: impl Into<String>, var_value: impl Into<String>) {
        self.env.insert(var_name.into(), var_value.into());
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Push {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub branches:        Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags:            Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub branches_ignore: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags_ignore:     Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub paths:           Vec<PathBuf>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub paths_ignore:    Vec<PathBuf>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PullRequest {}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Schedule {
    pub cron: String,
}

impl Schedule {
    pub fn new(cron_text: impl Into<String>) -> Result<Self> {
        let cron = cron_text.into();
        // Check if the given string is a valid cron expression.
        // let _ = cron::Schedule::from_str(cron_text.as_str())?;
        Ok(Self { cron })
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct WorkflowDispatch {}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Event {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push:              Option<Push>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pull_request:      Option<PullRequest>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub schedule:          Vec<Schedule>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_dispatch: Option<WorkflowDispatch>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Job {
    pub name:     String,
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub needs:    BTreeSet<String>,
    pub runs_on:  Vec<RunnerLabel>,
    pub steps:    Vec<Step>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub outputs:  BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy: Option<Strategy>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub env:      BTreeMap<String, String>,
}

impl Job {
    pub fn expose_output(&mut self, step_id: impl AsRef<str>, output_name: impl Into<String>) {
        let step = step_id.as_ref();
        let output = output_name.into();
        let value = format!("${{{{ steps.{step}.outputs.{output} }}}}");
        self.outputs.insert(output, value);
    }

    pub fn env(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.env.insert(name.into(), value.into());
    }

    pub fn expose_secret_as(&mut self, secret: impl AsRef<str>, given_name: impl Into<String>) {
        self.env(given_name, format!("${{{{ secrets.{} }}}}", secret.as_ref()));
    }

    pub fn use_job_outputs(&mut self, job_id: impl Into<String>, job: &Job) {
        let job_id = job_id.into();
        for (output_name, _) in &job.outputs {
            let reference = format!("${{{{needs.{}.outputs.{}}}}}", job_id, output_name);
            self.env.insert(output_name.into(), reference);
        }
        self.needs(job_id);
    }

    pub fn needs(&mut self, job_id: impl Into<String>) {
        self.needs.insert(job_id.into());
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Strategy {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub matrix:    BTreeMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fail_fast: Option<bool>,
}

impl Strategy {
    pub fn new_os(labels: impl Serialize) -> Strategy {
        let oses = serde_json::to_value(labels).unwrap();
        Strategy {
            fail_fast: Some(false),
            matrix:    [("os".to_string(), oses)].into_iter().collect(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Step {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id:    Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name:  Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uses:  Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run:   Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#if:  Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with:  Option<step::Argument>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub env:   BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<Shell>,
}

impl Step {
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn with_custom_argument(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        match &mut self.with {
            Some(step::Argument::Other(map)) => {
                map.insert(name.into(), value.into());
            }
            _ => {
                if let Some(previous) = self.with {
                    warn!("Dropping previous step argument: {:?}", previous);
                }
                self.with = Some(step::Argument::new_other(name, value));
            }
        }
        self
    }
}

pub fn github_token_env() -> (String, String) {
    ("GITHUB_TOKEN".into(), "${{ secrets.GITHUB_TOKEN }}".into())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Shell {
    /// Command Prompt.
    Cmd,
    Bash,
    /// Power Shell.
    Pwsh,
}

pub mod step {
    use super::*;


    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    #[serde(untagged)]
    pub enum Argument {
        #[serde(rename_all = "kebab-case")]
        Checkout {
            clean: Option<bool>,
        },
        #[serde(rename_all = "kebab-case")]
        SetupConda {
            #[serde(skip_serializing_if = "Option::is_none")]
            update_conda:   Option<bool>,
            #[serde(skip_serializing_if = "Option::is_none")]
            conda_channels: Option<String>, // conda_channels: Vec<CondaChannel>
        },
        #[serde(rename_all = "kebab-case")]
        GitHubScript {
            script: String,
        },
        Other(BTreeMap<String, String>),
    }

    impl Argument {
        pub fn new_other(name: impl Into<String>, value: impl Into<String>) -> Self {
            Argument::Other(BTreeMap::from_iter([(name.into(), value.into())]))
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RunnerLabel {
    #[serde(rename = "self-hosted")]
    SelfHosted,
    #[serde(rename = "macOS")]
    MacOS,
    #[serde(rename = "Linux")]
    Linux,
    #[serde(rename = "Windows")]
    Windows,
    #[serde(rename = "engine")]
    Engine,
    #[serde(rename = "macos-latest")]
    MacOSLatest,
    #[serde(rename = "linux-latest")]
    LinuxLatest,
    #[serde(rename = "windows-latest")]
    WindowsLatest,
    #[serde(rename = "X64")]
    X64,
    #[serde(rename = "mwu-deluxe")]
    MwuDeluxe,
    #[serde(rename = "${{ matrix.os }}")]
    MatrixOs,
}

pub fn checkout_repo_step() -> Step {
    Step {
        name: Some("Checking out the repository".into()),
        // FIXME: Check what is wrong with v3. Seemingly Engine Tests fail because there's only a
        //        shallow copy of the repo.
        uses: Some("actions/checkout@v2".into()),
        with: Some(step::Argument::Checkout { clean: Some(false) }),
        ..default()
    }
}

pub trait JobArchetype {
    fn id_key_base() -> String {
        std::any::type_name::<Self>().to_kebab_case()
    }

    fn key(os: OS) -> String {
        format!("{}-{}", Self::id_key_base(), os)
    }

    fn job(os: OS) -> Job;

    fn entry(os: OS) -> (String, Job) {
        (Self::key(os), Self::job(os))
    }

    // [Step ID] => [variable names]
    fn outputs() -> BTreeMap<String, Vec<String>> {
        default()
    }

    fn expose_outputs(job: &mut Job) {
        for (step_id, outputs) in Self::outputs() {
            for output in outputs {
                job.expose_output(&step_id, output);
            }
        }
    }
}
