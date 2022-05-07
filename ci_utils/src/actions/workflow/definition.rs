use crate::prelude::*;

use heck::ToKebabCase;
use std::collections::HashMap;

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

pub fn run(os: OS, command_line: impl AsRef<str>) -> Step {
    let bash_step = Step {
        run: Some(format!("./run.sh {}", command_line.as_ref())),
        // r#if: Some("runner.os != 'Windows'".into()),
        shell: Some(Shell::Bash),
        ..default()
    };

    let cmd_step = Step {
        run: Some(format!(r".\run.cmd {}", command_line.as_ref())),
        // r#if: Some("runner.os == 'Windows'".into()),
        shell: Some(Shell::Cmd),
        ..default()
    };
    if os == OS::Windows {
        cmd_step
    } else {
        bash_step
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobId(String);

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Workflow {
    pub name:        String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub on:          Event,
    pub jobs:        HashMap<String, Job>,
}

impl Workflow {
    pub fn add<J: JobArchetype>(&mut self, os: OS) -> String {
        self.add_customized::<J>(os, |_| {})
    }

    pub fn add_customized<J: JobArchetype>(&mut self, os: OS, f: impl FnOnce(&mut Job)) -> String {
        let (key, mut job) = J::entry(os);
        f(&mut job);
        self.jobs.insert(key.clone(), job);
        key
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Push {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    branches:        Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tags:            Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    branches_ignore: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tags_ignore:     Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    paths:           Vec<PathBuf>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    paths_ignore:    Vec<PathBuf>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Event {
    #[serde(skip_serializing_if = "Option::is_none")]
    push: Option<Push>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Job {
    pub name:    String,
    pub needs:   Vec<String>,
    pub runs_on: Vec<RunnerLabel>,
    pub steps:   Vec<Step>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Step {
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
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub env:   HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<Shell>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Shell {
    Cmd,
    Bash,
}

pub mod step {
    use super::*;


    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    #[serde(untagged)]
    pub enum Argument {
        Checkout {
            clean: Option<bool>,
        },
        SetupConda {
            #[serde(skip_serializing_if = "Option::is_none")]
            update_conda:   Option<bool>,
            #[serde(skip_serializing_if = "Option::is_none")]
            conda_channels: Option<String>, // conda_channels: Vec<CondaChannel>
        },
        GitHubScript {
            script: String,
        },
    }

    // #[derive(Clone, Debug, Serialize, Deserialize)]
    // #[serde(rename_all = "kebab-case")]
    // pub enum CondaChannel {
    //     Anaconda,
    //     CondaForge,
    // }
    // pub trait Argument: Clone + Debug + Serialize + DeserializeOwned + Sized {}
    //
    // #[derive(Clone, Debug, Serialize, Deserialize)]
    // pub struct CheckoutArgument {
    //     pub clean: Option<bool>,
    // }
    // impl Argument for CheckoutArgument {}
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
}

pub fn runs_on(os: OS) -> Vec<RunnerLabel> {
    match os {
        OS::Windows => vec![RunnerLabel::SelfHosted, RunnerLabel::Windows, RunnerLabel::Engine],
        OS::Linux => vec![RunnerLabel::SelfHosted, RunnerLabel::Linux, RunnerLabel::Engine],
        OS::MacOS => vec![RunnerLabel::MacOSLatest],
        _ => todo!("Not supported"),
    }
}

pub fn checkout_repo_step() -> Step {
    Step {
        name: Some("Checking out the repository".into()),
        uses: Some("actions/checkout@v3".into()),
        with: Some(step::Argument::Checkout { clean: Some(false) }),
        ..default()
    }
}

pub fn plain_job(os: OS, name: impl AsRef<str>, command_line: impl AsRef<str>) -> Job {
    let checkout_repo_step = checkout_repo_step();
    let run_step = run(os, command_line);
    let name = format!("{} ({})", name.as_ref(), os);
    let steps = vec![setup_conda(), setup_artifact_api(), checkout_repo_step, run_step];
    let runs_on = runs_on(os);
    Job { name, runs_on, steps, ..default() }
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
}

pub mod job {
    use super::*;

    pub struct Lint;
    impl JobArchetype for Lint {
        fn job(os: OS) -> Job {
            plain_job(os, "Lint", "lint")
        }
    }

    pub struct NativeTest;
    impl JobArchetype for NativeTest {
        fn job(os: OS) -> Job {
            plain_job(os, "Native GUI tests", "wasm test --no-wasm")
        }
    }

    pub struct WasmTest;
    impl JobArchetype for WasmTest {
        fn job(os: OS) -> Job {
            plain_job(os, "WASM GUI tests", "wasm test --no-native")
        }
    }

    pub struct IntegrationTest;
    impl JobArchetype for IntegrationTest {
        fn job(os: OS) -> Job {
            plain_job(os, "IDE integration tests", "ide integration-test")
        }
    }

    pub struct BuildWasm;
    impl JobArchetype for BuildWasm {
        fn job(os: OS) -> Job {
            plain_job(os, "Build GUI (WASM)", "wasm build")
        }
    }

    pub struct BuildProjectManager;
    impl JobArchetype for BuildProjectManager {
        fn job(os: OS) -> Job {
            plain_job(os, "Build Project Manager", "project-manager")
        }
    }

    pub struct PackageIde;
    impl JobArchetype for PackageIde {
        fn job(os: OS) -> Job {
            plain_job(
                os,
                "Package IDE",
                "ide build --wasm-source current-ci-run --project-manager-source current-ci-run",
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate() -> Result {
        let push_event = Push { ..default() };
        let mut workflow =
            Workflow { name: "GUI CI".into(), on: Event { push: Some(push_event) }, ..default() };

        let primary_os = OS::Linux;
        workflow.add::<job::Lint>(primary_os);
        workflow.add::<job::WasmTest>(primary_os);
        workflow.add::<job::NativeTest>(primary_os);
        workflow.add_customized::<job::IntegrationTest>(primary_os, |job| {
            job.needs.push(job::IntegrationTest::key(primary_os));
        });

        for os in [OS::Windows, OS::Linux, OS::MacOS] {
            let wasm_job = workflow.add::<job::BuildWasm>(os);
            let project_manager_job = workflow.add::<job::BuildProjectManager>(os);
            workflow.add_customized::<job::PackageIde>(os, |job| {
                job.needs.push(wasm_job);
                job.needs.push(project_manager_job);
            });
        }

        let yaml = serde_yaml::to_string(&workflow)?;
        println!("{yaml}");
        let path = r"H:\NBO\enso-staging\.github\workflows\gui.yml";
        crate::fs::write(path, yaml)?;
        Ok(())
    }
}


/*


name: 'Setup Enso Build'
description: 'Installs enso-build tool.'
inputs:
  clean:
    description: Whether the repository should be cleaned.
    required: true
    default: 'false'

#  enso_ref:
#    description: Reference to be cheked out in the Enso repository.
#    required: false
#    default: ''

runs:
  using: "composite"
  steps:
    - name: Setup conda (GH runners only)
      uses: s-weigand/setup-conda@v1.0.5
      if: startsWith(runner.name, 'GitHub Actions') || startsWith(runner.name, 'Hosted Agent') # GitHub-hosted runner.
      with:
        update-conda: false
        conda-channels: anaconda, conda-forge
    - name: Install wasm-pack (macOS GH runners only)
      env:
        WASMPACKURL: https://github.com/rustwasm/wasm-pack/releases/download/v0.10.2
        WASMPACKDIR: wasm-pack-v0.10.2-x86_64-apple-darwin
      run: |-
        curl -L "$WASMPACKURL/$WASMPACKDIR.tar.gz" | tar -xz -C .
        mv $WASMPACKDIR/wasm-pack ~/.cargo/bin
        rm -r $WASMPACKDIR
      shell: bash
      if: startsWith(runner.name, 'GitHub Actions') || startsWith(runner.name, 'Hosted Agent') # GitHub-hosted runner.
#    - uses: actions/checkout@v3
#      name: Checkout the repository
#      with:
#        clean: ${{ inputs.clean }}

    # Runs a set of commands using the runners shell
    - uses: actions/github-script@v6
      name: Setup the Artifact API environment
      with:
        script: |-
          core.exportVariable("ACTIONS_RUNTIME_TOKEN", process.env["ACTIONS_RUNTIME_TOKEN"])
          core.exportVariable("ACTIONS_RUNTIME_URL", process.env["ACTIONS_RUNTIME_URL"])
          core.exportVariable("GITHUB_RETENTION_DAYS", process.env["GITHUB_RETENTION_DAYS"])
    - run: ./run.sh --help
      shell: bash
      if: runner.os != 'Windows'
    - run: .\run.cmd --help
      shell: cmd
      if: runner.os == 'Windows'

 */

// lint:
// name: Lint
// runs-on: ${{ matrix.runner }}
// strategy:
// matrix:
// runner:
// #      - ["macos-latest"]
// #      - [Windows, self-hosted]
// - [Linux, self-hosted, engine]
// fail-fast: false
// steps:
// - uses: actions/checkout@v3
// name: Checkout the repository
// with:
// clean: false
// - uses: ./actions/setup-build
// - run: bash run.sh lint
