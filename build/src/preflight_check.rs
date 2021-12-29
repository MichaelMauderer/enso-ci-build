use crate::prelude::*;
use chrono::DateTime;
use chrono::Utc;
use octocrab::models::repos::Release;
use regex::Regex;
use semver::Prerelease;
use std::collections::BTreeSet;

const OWNER: &str = "enso-org";
const REPO: &str = "enso"; // FIXME
const MAX_PER_PAGE: u8 = 100;
const NIGHTLY_RELEASE_TITLE_INFIX: &str = "Nightly";

pub struct PreflightCheckOutput {
    pub proceed:         bool,
    pub enso_version:    Version,
    pub edition_version: Version,
}

pub fn is_nightly(release: &Release) -> bool {
    !release.draft
        && release.name.as_ref().map_or(false, |name| name.contains(NIGHTLY_RELEASE_TITLE_INFIX))
}

pub async fn nightly_releases(octocrab: &Octocrab) -> Result<Vec<Release>> {
    let repo = octocrab.repos(OWNER, REPO);
    let mut page = repo.releases().list().per_page(MAX_PER_PAGE).send().await?;
    // TODO: rate limit?
    let releases = octocrab.all_pages(page).await?.into_iter().filter(is_nightly);
    Ok(releases.collect())
}

/// Checks if there are any new changes to see if the nightly build should proceed.
pub fn check_proceed(current_head_sha: &str, nightlies: &[Release]) -> bool {
    if let Some(latest_nightly) = nightlies.first() {
        if latest_nightly.target_commitish == current_head_sha {
            println!("Current commit ({}) is the same as for the most recent nightly build. A new build is not needed.", current_head_sha);
            false
        } else {
            println!("Current commit ({}) is different from the most recent nightly build ({}). Proceeding with a new nightly build.", current_head_sha, latest_nightly.target_commitish);
            true
        }
    } else {
        println!("No prior nightly releases found. Proceeding with the first release.");
        true
    }
}

#[derive(Clone, Debug)]
pub struct Versions {
    engine:  Version,
    edition: String,
}

/// Prepares a version string and edition name for the nightly build.
///
/// A `-SNAPSHOT` suffix is added if it is not already present, next the current
/// date is appended. If this is not the first nightly build on that date, an
/// increasing numeric suffix is added.
pub fn prepare_version(
    date: DateTime<Utc>,
    repo_root: impl AsRef<Path>,
    nightlies: &[Release],
) -> Result<Versions> {
    let is_taken = |suffix: &str| nightlies.iter().any(|entry| entry.tag_name.ends_with(suffix));
    let build_sbt_path = repo_root.as_ref().join("build.sbt");
    let build_sbt_content = std::fs::read_to_string(&build_sbt_path)?;

    let found_version = enso_build::get_enso_version(&build_sbt_content)?;


    let date = date.format("%F").to_string();
    let generate_nightly_identifier = |index: u32| {
        if index == 0 {
            date.clone()
        } else {
            format!("{}.{}", date, index)
        }
    };


    let relevant_nightly_versions = nightlies
        .into_iter()
        .filter_map(|release| {
            if release.tag_name.contains(&date) {
                let version_str =
                    release.tag_name.strip_prefix("enso-").unwrap_or(&release.tag_name);
                Version::parse(version_str).ok().map(|v| v.pre)
            } else {
                None
            }
        })
        .collect::<BTreeSet<_>>();


    for index in 0.. {
        let nightly = generate_nightly_identifier(index);
        let prerelease_text = format!("SNAPSHOT.{}", nightly);
        let pre = Prerelease::new(&prerelease_text)?;
        if !relevant_nightly_versions.contains(&pre) {
            let edition = format!("nightly-{}", nightly);
            let mut engine = Version { pre, ..found_version };
            return Ok(Versions { engine, edition });
        }
    }

    // After infinite loop.
    unreachable!()
}


// async function main() {
//     const nightlies = await github.fetchNightlies()
//     const shouldProceed = checkProceed(nightlies)
//     setProceed(shouldProceed)
//     if (shouldProceed) {
//         const versions = prepareVersions(nightlies)
//         setVersionString(versions.version)
//         setEditionName(versions.edition)
//     }
// }
//
// main().catch(err => {
//     console.error(err)
//     process.exit(1)
// })


#[cfg(test)]
mod tests {
    use super::*;
    use ide_ci::programs::git::Git;

    #[tokio::test]
    async fn foo() -> Result {
        let octocrab = Octocrab::default();
        let repo_path = PathBuf::from(r"H:\NBO\enso");
        let git = Git::new(&repo_path);
        let nightlies = nightly_releases(&octocrab).await?;

        let proceed = check_proceed(&git.head_hash().await?, &nightlies);
        ide_ci::actions::workflow::set_output("proceed", proceed);
        if proceed {
            let date = chrono::Utc::now();
            let versions = prepare_version(date, &repo_path, &nightlies)?;
            ide_ci::actions::workflow::set_output("nightly-version", &versions.engine);
            ide_ci::actions::workflow::set_output("nightly-edition", &versions.edition);
        }
        Ok(())
    }
}
