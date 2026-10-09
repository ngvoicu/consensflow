//! The shell scripts the workflows run, parsed. A hand run of a workflow shows
//! a step's syntax only once it gets there (the publish step never, short of a
//! tag), so a slip in one is found on release day; `bash -n` finds it here.
//! PowerShell's steps (`shell: pwsh`) are the Windows runner's to parse.

// A failure in a test is the test's answer, in its helpers too.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::process::Command;

use cf_publish::testing::workflow::{bash_steps, files, has_bash, parses_as_bash, read};

mod the_scripts_of_the_workflows {
    use super::*;

    #[test]
    fn are_read_off_every_workflow_the_publish_step_among_them() {
        if !has_bash() {
            eprintln!("there is no bash here");
            return;
        }
        let release = bash_steps(&read("release.yml"));
        assert!(
            release.len() >= 10,
            "release.yml: {} bash steps found",
            release.len()
        );
        for name in [
            "The old feeds serve the bridge, for a release after it",
            "Notes, and the update feed for this build",
            "Build cf-publish, the rule of the feeds",
            "Build cf-publish, and say what it hashes to",
            "The publisher is the binary that was built",
            "Publish the release, then its update feeds",
            "The feeds serve this release, and its archive downloads",
        ] {
            assert!(
                release.iter().any(|step| step.name == name),
                "release.yml has no bash step named {name}"
            );
        }
        let windows = bash_steps(&read("windows-build.yml"));
        assert!(
            windows
                .iter()
                .all(|step| !step.name.contains("portable exe")),
            "the PowerShell step is not read as bash"
        );
    }

    #[test]
    fn all_parse_as_bash_bash_n() {
        if !has_bash() {
            eprintln!("there is no bash here");
            return;
        }
        for file in files() {
            for step in bash_steps(&read(&file)) {
                if let Err(said) = parses_as_bash(&step.script) {
                    panic!("{file}: {}\n{said}", step.name);
                }
            }
        }
    }
}

mod the_workflows_as_yaml {
    use std::fs;

    use cf_publish::testing::TempDir;

    use super::*;

    /// What ruby is asked to do with each workflow: parse it, and find its jobs.
    const PARSE: &str = r##"require 'yaml'
ARGV.each do |path|
  document = YAML.safe_load(File.read(path))
  jobs = document.fetch('jobs')
  raise "#{path}: no jobs" if jobs.empty?
  jobs.each do |name, job|
    next if job.key?('uses')
    raise "#{path}: #{name} has no steps" unless job['steps'].is_a?(Array) && !job['steps'].empty?
  end
end
"##;

    /// Ruby's YAML (libyaml), the parser a machine here has without a crate or a package:
    /// every workflow must parse, with the jobs and the steps GitHub reads. Where there is
    /// no ruby the check is not made, and says so.
    #[test]
    #[allow(clippy::disallowed_methods)] // The test starts what it tests with.
    fn every_workflow_is_valid_yaml_with_jobs_that_have_steps_or_call_a_workflow() {
        let dir = TempDir::new("yaml");
        let script = dir.path().join("parse.rb");
        fs::write(&script, PARSE).unwrap();
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(".github")
            .join("workflows");
        let paths: Vec<_> = files().iter().map(|file| folder.join(file)).collect();
        let ran = match Command::new("ruby").arg(&script).args(&paths).output() {
            Ok(ran) => ran,
            Err(_) => {
                eprintln!("there is no ruby here: the workflows are not parsed as YAML");
                return;
            }
        };
        assert!(
            ran.status.success(),
            "{}{}",
            String::from_utf8_lossy(&ran.stdout),
            String::from_utf8_lossy(&ran.stderr)
        );
    }
}
