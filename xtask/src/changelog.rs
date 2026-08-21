use std::fmt::Write as _;
use std::fs;
use std::path::Path;

pub(crate) const FILE_NAME: &str = "CHANGELOG.md";

pub(crate) fn prepare(
    path: &Path,
    current_version: &str,
    next_version: &str,
    release_date: &str,
) -> Result<String, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("could not read '{}': {error}", path.display()))?;
    promote(&contents, current_version, next_version, release_date)
}

pub(crate) fn write(path: &Path, contents: String) -> Result<(), String> {
    fs::write(path, contents)
        .map_err(|error| format!("could not update '{}': {error}", path.display()))
}

pub(crate) fn validate(path: &Path, version: &str) -> Result<(), String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("could not read '{}': {error}", path.display()))?;
    validate_contents(&contents, version)
}

fn promote(
    contents: &str,
    current_version: &str,
    next_version: &str,
    release_date: &str,
) -> Result<String, String> {
    let unreleased_heading = "## [Unreleased]";
    let heading_start = contents
        .find(unreleased_heading)
        .ok_or_else(|| format!("{FILE_NAME} has no [Unreleased] section"))?;
    let body_start = heading_start + unreleased_heading.len();
    let remainder = &contents[body_start..];
    let next_heading = remainder
        .find("\n## [")
        .ok_or_else(|| format!("{FILE_NAME} has no release section after [Unreleased]"))?;
    let unreleased_body = remainder[..next_heading].trim();
    if unreleased_body.is_empty() {
        return Err(format!("{FILE_NAME} [Unreleased] section is empty"));
    }
    if contents
        .lines()
        .any(|line| line.starts_with(&format!("## [{next_version}] - ")))
    {
        return Err(format!(
            "{FILE_NAME} already contains release {next_version}"
        ));
    }

    let mut promoted = String::with_capacity(contents.len() + 64);
    promoted.push_str(&contents[..heading_start]);
    promoted.push_str(unreleased_heading);
    promoted.push_str("\n\n");
    let _ = writeln!(promoted, "## [{next_version}] - {release_date}\n");
    promoted.push_str(unreleased_body);
    promoted.push('\n');
    promoted.push_str(&remainder[next_heading..]);

    replace_links(&promoted, current_version, next_version)
}

fn replace_links(
    contents: &str,
    current_version: &str,
    next_version: &str,
) -> Result<String, String> {
    let prefix = "[Unreleased]: ";
    let expected_suffix = format!("/compare/v{current_version}...HEAD");
    let mut replaced = false;
    let mut updated = String::with_capacity(contents.len() + 128);

    for line in contents.split_inclusive('\n') {
        let line_without_newline = line.strip_suffix('\n').unwrap_or(line);
        if let Some(url) = line_without_newline.strip_prefix(prefix) {
            if replaced {
                return Err(format!(
                    "{FILE_NAME} contains more than one [Unreleased] comparison link"
                ));
            }
            let repository_url = url.strip_suffix(&expected_suffix).ok_or_else(|| {
                format!("{FILE_NAME} [Unreleased] link must compare v{current_version} to HEAD")
            })?;
            let _ = writeln!(
                updated,
                "[Unreleased]: {repository_url}/compare/v{next_version}...HEAD"
            );
            let _ = writeln!(
                updated,
                "[{next_version}]: {repository_url}/compare/v{current_version}...v{next_version}"
            );
            replaced = true;
        } else {
            updated.push_str(line);
        }
    }

    if replaced {
        Ok(updated)
    } else {
        Err(format!("{FILE_NAME} has no [Unreleased] comparison link"))
    }
}

fn validate_contents(contents: &str, version: &str) -> Result<(), String> {
    let heading_prefix = format!("## [{version}] - ");
    let mut headings = contents
        .match_indices(&heading_prefix)
        .filter(|(index, _)| *index == 0 || contents.as_bytes()[index - 1] == b'\n');
    let (heading_start, _) = headings
        .next()
        .ok_or_else(|| format!("{FILE_NAME} has no release section for {version}"))?;
    if headings.next().is_some() {
        return Err(format!(
            "{FILE_NAME} contains more than one release section for {version}"
        ));
    }
    let heading_end = contents[heading_start..]
        .find('\n')
        .map_or(contents.len(), |offset| heading_start + offset);
    let release_date = contents[heading_start + heading_prefix.len()..heading_end].trim();
    let date_format = time::format_description::parse_borrowed::<3>("[year]-[month]-[day]")
        .expect("release date format should be valid");
    time::Date::parse(release_date, &date_format).map_err(|error| {
        format!("{FILE_NAME} release {version} has invalid date '{release_date}': {error}")
    })?;

    let release_body_start = heading_end.saturating_add(1);
    let release_remainder = &contents[release_body_start..];
    let release_body_end = release_remainder
        .find("\n## [")
        .unwrap_or(release_remainder.len());
    let release_body = &release_remainder[..release_body_end];
    if !release_body.lines().any(|line| line.starts_with("- ")) {
        return Err(format!("{FILE_NAME} release {version} has no entries"));
    }

    let unreleased_suffix = format!("/compare/v{version}...HEAD");
    let mut unreleased_links = contents
        .lines()
        .filter_map(|line| line.strip_prefix("[Unreleased]: "));
    let unreleased_link = unreleased_links
        .next()
        .ok_or_else(|| format!("{FILE_NAME} has no [Unreleased] comparison link"))?;
    if unreleased_links.next().is_some() {
        return Err(format!(
            "{FILE_NAME} contains more than one [Unreleased] comparison link"
        ));
    }
    if !unreleased_link.ends_with(&unreleased_suffix) {
        return Err(format!(
            "{FILE_NAME} [Unreleased] link must compare v{version} to HEAD"
        ));
    }

    let version_link_prefix = format!("[{version}]: ");
    let mut version_links = contents
        .lines()
        .filter_map(|line| line.strip_prefix(&version_link_prefix));
    let version_link = version_links
        .next()
        .ok_or_else(|| format!("{FILE_NAME} has no link for release {version}"))?;
    if version_links.next().is_some() {
        return Err(format!(
            "{FILE_NAME} contains more than one link for release {version}"
        ));
    }
    if !version_link.ends_with(&format!("...v{version}"))
        && !version_link.ends_with(&format!("/tag/v{version}"))
    {
        return Err(format!(
            "{FILE_NAME} release link does not target v{version}"
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{promote, validate_contents};

    #[test]
    fn promotion_preserves_notes_and_updates_comparison_links() {
        let changelog = concat!(
            "# Changelog\n\n",
            "## [Unreleased]\n\n",
            "### Fixed\n\n",
            "- A user-visible bug.\n\n",
            "## [0.0.1] - 2026-08-20\n\n",
            "### Added\n\n",
            "- Initial release.\n\n",
            "[Unreleased]: https://github.com/example/project/compare/v0.0.1...HEAD\n",
            "[0.0.1]: https://github.com/example/project/releases/tag/v0.0.1\n",
        );

        let updated = promote(changelog, "0.0.1", "0.0.2", "2026-08-21")
            .expect("changelog should be promoted");

        assert!(updated.contains("## [Unreleased]\n\n## [0.0.2] - 2026-08-21"));
        assert!(updated.contains("- A user-visible bug."));
        assert!(
            updated
                .contains("[Unreleased]: https://github.com/example/project/compare/v0.0.2...HEAD")
        );
        assert!(
            updated.contains("[0.0.2]: https://github.com/example/project/compare/v0.0.1...v0.0.2")
        );
        validate_contents(&updated, "0.0.2").expect("promoted changelog should be valid");
    }

    #[test]
    fn promotion_rejects_empty_unreleased_notes() {
        let changelog = concat!(
            "# Changelog\n\n",
            "## [Unreleased]\n\n",
            "## [0.0.1] - 2026-08-20\n\n",
            "- Initial release.\n\n",
            "[Unreleased]: https://github.com/example/project/compare/v0.0.1...HEAD\n",
            "[0.0.1]: https://github.com/example/project/releases/tag/v0.0.1\n",
        );

        let error = promote(changelog, "0.0.1", "0.0.2", "2026-08-21")
            .expect_err("empty notes should be rejected");

        assert!(error.contains("[Unreleased] section is empty"));
    }

    #[test]
    fn validation_requires_release_entries() {
        let changelog = concat!(
            "# Changelog\n\n",
            "## [Unreleased]\n\n",
            "## [0.0.2] - 2026-08-21\n\n",
            "### Fixed\n\n",
            "## [0.0.1] - 2026-08-20\n\n",
            "- Initial release.\n\n",
            "[Unreleased]: https://github.com/example/project/compare/v0.0.2...HEAD\n",
            "[0.0.2]: https://github.com/example/project/compare/v0.0.1...v0.0.2\n",
            "[0.0.1]: https://github.com/example/project/releases/tag/v0.0.1\n",
        );

        let error = validate_contents(changelog, "0.0.2")
            .expect_err("release without entries should be rejected");

        assert!(error.contains("release 0.0.2 has no entries"));
    }
}
