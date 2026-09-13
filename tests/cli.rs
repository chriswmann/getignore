use assert_cmd::{Command, cargo::*};
use assert_fs::{TempDir, fixture::PathChild};
use predicates::{prelude::*, str::contains};

const ALIASES: [&str; 31] = [
    "js",
    "javascript",
    "ts",
    "typescript",
    "nodejs",
    "c#",
    "csharp",
    "f#",
    "fsharp",
    ".net",
    "cpp",
    "cxx",
    "objc",
    "objectivec",
    "py",
    "rb",
    "rs",
    "golang",
    "hs",
    "kt",
    "ex",
    "erl",
    "clj",
    "pl",
    "ml",
    "latex",
    "php",
    "unity3d",
    "unreal",
    "ue4",
    "ue5",
];

/// run `gi` in a temp dir, with the cache pointing to the temp dir as well
fn gi(temp: &TempDir) -> Command {
    let mut cmd = cargo_bin_cmd!("gi");
    cmd.current_dir(temp.path())
        .env("XDG_CACHE_HOME", temp.child("cache").path());
    cmd
}

#[test]
fn test_version_flag() {
    cargo_bin_cmd!("gi").arg("--version").assert().success();
}

#[test]
fn aliases_match_correct_template() {
    let temp = TempDir::new().unwrap();
    for alias in ALIASES {
        gi(&temp).arg(alias).assert().success();
    }
}

#[test]
fn query_that_does_not_match_anything_exits_with_code_1() {
    let temp = TempDir::new().unwrap();
    gi(&temp).arg("does not match anything").assert().code(1);
}

#[test]
fn query_coldb_matches_two_templates() {
    let temp = TempDir::new().unwrap();
    gi(&temp).arg("coldb").assert().code(1).stderr(
        contains("community/BoxLang/ColdBox.gitignore")
            .and(contains("community/CFML/ColdBox.gitignore")),
    );
}

#[test]
fn query_haskel_matches_haskel_template() {
    let temp = TempDir::new().unwrap();
    gi(&temp)
        .arg("haskel")
        .assert()
        .code(0)
        .stdout(contains("Haskell.gitignore"));
}
