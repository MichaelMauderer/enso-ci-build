use crate::prelude::*;
use byte_unit::Byte;
use ide_ci::program;
use semver::VersionReq;

pub fn load_yaml(yaml_text: &str) -> Result<Config> {
    let raw = serde_yaml::from_str::<ConfigRaw>(yaml_text)?;
    raw.try_into()
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, strum::EnumString)]
pub enum RecognizedProgram {
    #[strum(default)]
    Other(String),
}

impl RecognizedProgram {
    pub async fn version(&self) -> Result<Version> {
        match self {
            RecognizedProgram::Other(program) => program::Unknown(program.clone()).version().await,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ConfigRaw {
    pub wasm_size_limit:   Option<String>,
    pub required_versions: HashMap<String, String>,
}

#[derive(Clone, Debug, Default)]
pub struct Config {
    pub wasm_size_limit:   Option<Byte>,
    pub required_versions: HashMap<RecognizedProgram, VersionReq>,
}

impl Config {
    pub async fn check_programs(&self) -> Result {
        for (program, version_req) in &self.required_versions {
            let found = program.version().await?;
            if !version_req.matches(&found) {
                bail!(
                    "Found program {:?} in version {} that does not fulfill requirement {}.",
                    program,
                    found,
                    version_req
                );
            } else {
                info!(
                    "Found program {:?} in supported version {} (required {}).",
                    program, found, version_req
                );
            }
        }
        Ok(())
    }
}

impl TryFrom<ConfigRaw> for Config {
    type Error = anyhow::Error;

    fn try_from(value: ConfigRaw) -> std::result::Result<Self, Self::Error> {
        let mut required_versions = HashMap::new();
        for (program, version_req) in value.required_versions {
            required_versions.insert(
                <RecognizedProgram as FromString>::from_str(&program)?,
                <VersionReq as FromString>::from_str(&version_req)?,
            );
        }

        Ok(Self {
            wasm_size_limit: value
                .wasm_size_limit
                .map(|limit_text| <Byte as FromString>::from_str(&limit_text))
                .transpose()?,
            required_versions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_ci::log::setup_logging;

    #[tokio::test]
    async fn deserialize() -> Result {
        setup_logging()?;
        let config = r#"
# Options intended to be common for all developers.
wasm-size-limit: "4.37MB"
required-versions:
  node: =16.15.0
  wasm-pack: ^0.10.2
  flatc: =1.12.0
"#;
        let config = serde_yaml::from_str::<ConfigRaw>(config)?;
        dbg!(&config);
        dbg!(Config::try_from(config))?.check_programs().await?;


        Ok(())
    }
}
