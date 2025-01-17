use crate::prelude::*;

pub struct VsWhere;

impl Program for VsWhere {
    fn default_locations(&self) -> Vec<PathBuf> {
        let dir_opt = crate::platform::win::program_files_x86()
            .map(|program_files| program_files.join("Microsoft Visual Studio").join("Installer"));
        Vec::from_iter(dir_opt)
    }

    fn executable_name(&self) -> &'static str {
        "vswhere"
    }
}

impl VsWhere {
    pub async fn find_all_with(component: Component) -> Result<Vec<InstanceInfo>> {
        let mut command = VsWhere.cmd()?;
        command
            .args(Option::Format(Format::Json).format_arguments())
            .args(Option::Required(vec![component]).format_arguments())
            .args(Option::ForceUTF8.format_arguments());

        let stdout = command.run_stdout().await?;
        serde_json::from_str(&stdout).anyhow_err()
    }

    pub async fn find_with(component: Component) -> Result<InstanceInfo> {
        let mut command = VsWhere.cmd()?;
        command
            .args(Option::Format(Format::Json).format_arguments())
            .args(Option::Required(vec![component]).format_arguments())
            .args(Option::ForceUTF8.format_arguments())
            .args(["-products", "*"]); // FIXME add types

        let stdout = command.run_stdout().await?;
        let instances = serde_json::from_str::<Vec<InstanceInfo>>(&stdout)?;
        Ok(instances.into_iter().next().ok_or(NoMsvcInstallation)?)
    }

    /// Looks up installation of Visual Studio that has installed
    /// `MSVC v142 - VS 2019 C++ x64/x86 build tools (v14.28)` component.
    /// E.g. "C:\Program Files (x86)\Microsoft Visual Studio\2019\Community"
    pub async fn msvc() -> Result<InstanceInfo> {
        Self::find_with(Component::CppBuildTools).await
    }

    pub async fn with_msbuild() -> Result<InstanceInfo> {
        Self::find_with(Component::MsBuild).await
    }
}

#[derive(Clone, Copy, Debug, Snafu)]
#[snafu(display("failed to find a MSVC installation"))]
pub struct NoMsvcInstallation;

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InstanceInfo {
    pub install_date:         chrono::DateTime<chrono::Utc>,
    /// Example: C:\\Program Files\\Microsoft Visual Studio\\2022\\Community
    pub installation_path:    PathBuf,
    pub installation_version: String,
    pub is_prerelease:        bool,
    pub display_name:         String,
    pub catalog:              Catalog,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub product_line_version: crate::programs::vs::Version,
    /* "buildBranch": "d16.8",
     * "buildVersion": "16.8.30711.63",
     * "id": "VisualStudio/16.8.1+30711.63",
     * "localBuild": "build-lab",
     * "manifestName": "VisualStudio",
     * "manifestType": "installer",
     * "productDisplayVersion": "16.8.1",
     * "productLine": "Dev16",
     * "productMilestone": "RTW",
     * "productMilestoneIsPreRelease": "False",
     * "productName": "Visual Studio",
     * "productPatchVersion": "1",
     * "productPreReleaseMilestoneSuffix": "1.0",
     * "productSemanticVersion": "16.8.1+30711.63",
     * "requiredEngineVersion": "2.8.3267.30329" */
}

pub enum Option {
    /// Output format.
    Format(Format),
    /// One or more workload or component IDs required when finding instances.
    /// All specified IDs must be installed unless -requiresAny is specified.
    Required(Vec<Component>),
    /// Forces output to be written as UTF-8, regardless of the code page.
    ForceUTF8,
}

impl Option {
    fn format_arguments(&self) -> Vec<OsString> {
        match self {
            Self::Format(fmt) => vec!["-format".into(), fmt.into()],
            Self::Required(components) => {
                let mut args = vec!["-requires".into()];
                for component in components {
                    args.push(component.into())
                }
                args
            }
            Self::ForceUTF8 => vec!["-utf8".into()],
        }
    }
}

pub enum Format {
    Json,
    Text,
    Value,
    Xml,
}

impl Into<OsString> for &Format {
    fn into(self) -> OsString {
        match self {
            Format::Json => "json",
            Format::Text => "text",
            Format::Value => "value",
            Format::Xml => "xml",
        }
        .into()
    }
}

// cf. https://docs.microsoft.com/en-us/visualstudio/install/workload-component-id-vs-community?view=vs-2019&preserve-view=true
pub enum Component {
    /// MSVC v142 - VS 2019 C++ x64/x86 build tools
    CppBuildTools,
    /// MSBuild
    MsBuild,
}

impl From<&Component> for OsString {
    fn from(value: &Component) -> Self {
        match value {
            // cf. https://docs.microsoft.com/en-us/visualstudio/install/workload-component-id-vs-community?view=vs-2019&preserve-view=true
            Component::CppBuildTools => "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            Component::MsBuild => "Microsoft.Component.MSBuild",
        }
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn vswhere() {
        let _ = dbg!(VsWhere::msvc().await);
    }

    #[test]
    fn parse() {
        let sample_out = r#"
[
  {
    "instanceId": "a7578c88",
    "installDate": "2019-04-02T19:34:05Z",
    "installationName": "VisualStudio/16.8.1+30711.63",
    "installationPath": "C:\\Program Files (x86)\\Microsoft Visual Studio\\2019\\Community",
    "installationVersion": "16.8.30711.63",
    "productId": "Microsoft.VisualStudio.Product.Community",
    "productPath": "C:\\Program Files (x86)\\Microsoft Visual Studio\\2019\\Community\\Common7\\IDE\\devenv.exe",
    "state": 4294967295,
    "isComplete": true,
    "isLaunchable": true,
    "isPrerelease": false,
    "isRebootRequired": false,
    "displayName": "Visual Studio Community 2019",
    "description": "Zaawansowane środowisko IDE — bezpłatne dla uczniów i studentów, współautorów oprogramowania open source oraz indywidualnych osób",
    "channelId": "VisualStudio.16.Release",
    "channelUri": "https://aka.ms/vs/16/release/channel",
    "enginePath": "C:\\Program Files (x86)\\Microsoft Visual Studio\\Installer\\resources\\app\\ServiceHub\\Services\\Microsoft.VisualStudio.Setup.Service",
    "releaseNotes": "https://go.microsoft.com/fwlink/?LinkId=660893#16.8.1",
    "thirdPartyNotices": "https://go.microsoft.com/fwlink/?LinkId=660909",
    "updateDate": "2020-11-12T21:48:39.0758481Z",
    "catalog": {
      "buildBranch": "d16.8",
      "buildVersion": "16.8.30711.63",
      "id": "VisualStudio/16.8.1+30711.63",
      "localBuild": "build-lab",
      "manifestName": "VisualStudio",
      "manifestType": "installer",
      "productDisplayVersion": "16.8.1",
      "productLine": "Dev16",
      "productLineVersion": "2019",
      "productMilestone": "RTW",
      "productMilestoneIsPreRelease": "False",
      "productName": "Visual Studio",
      "productPatchVersion": "1",
      "productPreReleaseMilestoneSuffix": "1.0",
      "productSemanticVersion": "16.8.1+30711.63",
      "requiredEngineVersion": "2.8.3267.30329"
    },
    "properties": {
      "campaignId": "535420412.1544277453",
      "channelManifestId": "VisualStudio.16.Release/16.8.1+30711.63",
      "nickname": "",
      "setupEngineFilePath": "C:\\Program Files (x86)\\Microsoft Visual Studio\\Installer\\vs_installershell.exe"
    }
  },
  {
    "instanceId": "aa771714",
    "installDate": "2018-12-08T14:06:40Z",
    "installationName": "VisualStudio/15.9.15+28307.812",
    "installationPath": "C:\\Program Files (x86)\\Microsoft Visual Studio\\2017\\Community",
    "installationVersion": "15.9.28307.812",
    "productId": "Microsoft.VisualStudio.Product.Community",
    "productPath": "C:\\Program Files (x86)\\Microsoft Visual Studio\\2017\\Community\\Common7\\IDE\\devenv.exe",
    "state": 4294967295,
    "isComplete": true,
    "isLaunchable": true,
    "isPrerelease": false,
    "isRebootRequired": false,
    "displayName": "Visual Studio Community 2017",
    "description": "Bezpłatne, w pełni funkcjonalne środowisko IDE dla studentów oraz programistów indywidualnych i tworzących rozwiązania open source",
    "channelId": "VisualStudio.15.Release",
    "channelUri": "https://aka.ms/vs/15/release/channel",
    "enginePath": "C:\\Program Files (x86)\\Microsoft Visual Studio\\Installer\\resources\\app\\ServiceHub\\Services\\Microsoft.VisualStudio.Setup.Service",
    "releaseNotes": "https://go.microsoft.com/fwlink/?LinkId=660692#15.9.15",
    "thirdPartyNotices": "https://go.microsoft.com/fwlink/?LinkId=660708",
    "updateDate": "2019-08-15T16:21:01.6235246Z",
    "catalog": {
      "buildBranch": "d15.9",
      "buildVersion": "15.9.28307.812",
      "id": "VisualStudio/15.9.15+28307.812",
      "localBuild": "build-lab",
      "manifestName": "VisualStudio",
      "manifestType": "installer",
      "productDisplayVersion": "15.9.15",
      "productLine": "Dev15",
      "productLineVersion": "2017",
      "productMilestone": "RTW",
      "productMilestoneIsPreRelease": "False",
      "productName": "Visual Studio",
      "productPatchVersion": "15",
      "productPreReleaseMilestoneSuffix": "1.0",
      "productRelease": "RTW",
      "productSemanticVersion": "15.9.15+28307.812",
      "requiredEngineVersion": "1.18.1049.33485"
    },
    "properties": {
      "campaignId": "535420412.1544277453",
      "channelManifestId": "VisualStudio.15.Release/15.9.15+28307.812",
      "nickname": "",
      "setupEngineFilePath": "C:\\Program Files (x86)\\Microsoft Visual Studio\\Installer\\vs_installershell.exe"
    }
  }
]"#;
        let ret = serde_json::from_str::<Vec<InstanceInfo>>(sample_out);
        assert!(ret.is_ok());
    }
}
