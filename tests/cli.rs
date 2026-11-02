use std::{fs, path::Path, process::Output};

use serde_json::Value;

fn run(directory: &Path, args: &[&str]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_outbound-manifest"))
        .current_dir(directory)
        .args(args)
        .output()
        .unwrap()
}

fn json(output: &Output) -> Value {
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn fixture() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("task")).unwrap();
    fs::write(
        directory.path().join("task/body.md"),
        "- 判断材料: TODO\n\n---\n\nHello,\n",
    )
    .unwrap();
    fs::write(directory.path().join("task/doc.pdf"), b"%PDF-1.4 fixture").unwrap();
    directory
}

const CREATE: &[&str] = &[
    "manifest",
    "create-manifest",
    "--manifest-file",
    "task/outbound.json",
    "--channel",
    "email",
    "--context",
    "Gmail sender@example.com / thread",
    "--to",
    "a@example.com",
    "b@example.com",
    "--subject",
    "Documents",
    "--body-file",
    "body.md",
    "--attachments",
    "doc.pdf",
];

const MANIFEST: &[&str] = &["--manifest-file", "task/outbound.json"];

fn with(prefix: &[&'static str], rest: &[&'static str]) -> Vec<&'static str> {
    prefix.iter().chain(rest).copied().collect()
}

#[test]
fn create_check_review_seal_and_verify() {
    let directory = fixture();
    let dir = directory.path();
    let created = json(&run(dir, CREATE));
    assert_eq!(created["Recipients"], 2);
    assert_eq!(created["Sealed"], false);
    let content = created["ContentSha256"].clone();

    let checked = json(&run(dir, &with(&["manifest", "check-manifest"], MANIFEST)));
    assert_eq!(checked["ContentSha256"], content);

    let review = run(
        dir,
        &with(
            &["manifest", "review-manifest", "--output", "text"],
            MANIFEST,
        ),
    );
    let review = String::from_utf8(review.stdout).unwrap();
    assert!(review.starts_with("# External action review\n"));
    assert!(review.contains("- To: a@example.com, b@example.com\n"));
    assert!(review.contains("\nHello,\n"));
    assert!(!review.contains("判断材料"));

    let unsealed = run(dir, &with(&["manifest", "verify-manifest"], MANIFEST));
    assert_eq!(unsealed.status.code(), Some(254));

    let sealed = json(&run(
        dir,
        &with(
            &[
                "manifest",
                "seal-manifest",
                "--approval-note",
                "chat: 送って",
            ],
            MANIFEST,
        ),
    ));
    assert_eq!(sealed["ContentSha256"], content);
    assert_eq!(sealed["ApprovedBy"], "user");

    let verified = json(&run(dir, &with(&["manifest", "verify-manifest"], MANIFEST)));
    assert_eq!(verified["Sealed"], true);

    fs::write(dir.join("task/body.md"), "- 判断材料\n\n---\n\nHello!\n").unwrap();
    let changed = run(dir, &with(&["manifest", "verify-manifest"], MANIFEST));
    assert_eq!(changed.status.code(), Some(254));
    assert!(String::from_utf8_lossy(&changed.stderr).contains("approved content changed"));
}

#[test]
fn a_manifest_sealed_by_the_earlier_format_still_verifies() {
    let directory = tempfile::tempdir().unwrap();
    let dir = directory.path();
    fs::write(
        dir.join("body.md"),
        "- 宛先メモ\n\n---\n\nHello,\n\n本文です。\n",
    )
    .unwrap();
    fs::write(dir.join("a.pdf"), b"%PDF-1.4 x").unwrap();
    fs::write(
        dir.join("m.json"),
        r#"{
  "schema": 1,
  "channel": "email",
  "context": "Gmail a@example.com / test",
  "to": ["p@example.com"],
  "cc": [],
  "bcc": [],
  "subject": "Docs",
  "body_file": "body.md",
  "attachments": [
    {"path": "a.pdf", "sha256": "fc022288f3e49d09353987de1c32fc1427bd2a8b3657a340e86efb8e34efe003"}
  ],
  "allow_placeholders": [],
  "approval": {
    "by": "user",
    "approved_at": "2026-11-02T21:31:16.023256391Z",
    "note": "old seal",
    "content_sha256": "65ce9c6482b4bc1d350ddd5527b8d6c9ab085d3fceac4c8afcf4cba6a6c9d2a9"
  }
}
"#,
    )
    .unwrap();
    let verified = json(&run(
        dir,
        &["manifest", "verify-manifest", "--manifest-file", "m.json"],
    ));
    assert_eq!(
        verified["ContentSha256"],
        "65ce9c6482b4bc1d350ddd5527b8d6c9ab085d3fceac4c8afcf4cba6a6c9d2a9"
    );
}

#[test]
fn help_is_available_at_every_level() {
    let directory = tempfile::tempdir().unwrap();
    for args in [
        vec!["help"],
        vec!["manifest", "help"],
        vec!["manifest", "seal-manifest", "help"],
    ] {
        let output = run(directory.path(), &args);
        assert_eq!(output.status.code(), Some(0), "{args:?}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("Usage:"));
    }
    let output = run(directory.path(), &["manifest", "seal-manifest", "help"]);
    assert!(String::from_utf8_lossy(&output.stdout).contains("--approval-note"));
}

#[test]
fn syntax_errors_exit_252() {
    let directory = tempfile::tempdir().unwrap();
    for args in [
        vec![],
        vec!["manifest", "check-manifest"],
        vec!["check", "--manifest", "m.json"],
        vec![
            "manifest",
            "check-manifest",
            "--manifest-file",
            "file://missing",
        ],
        vec![
            "manifest",
            "create-manifest",
            "--cli-input-json",
            "{\"Nope\": 1}",
        ],
    ] {
        assert_eq!(
            run(directory.path(), &args).status.code(),
            Some(252),
            "{args:?}"
        );
    }
}

#[test]
fn an_invalid_query_stops_before_the_manifest_is_written() {
    let directory = fixture();
    let output = run(directory.path(), &with(CREATE, &["--query", "[["]));
    assert_eq!(output.status.code(), Some(252));
    assert!(!directory.path().join("task/outbound.json").exists());
}

#[test]
fn cli_input_json_and_file_values_fill_parameters() {
    let directory = fixture();
    let dir = directory.path();
    fs::write(dir.join("context.txt"), "Slack #general").unwrap();
    fs::write(
        dir.join("input.json"),
        r#"{"ManifestFile": "task/outbound.json", "Channel": "slack", "Context": "overridden", "To": [], "BodyFile": "body.md", "Attachments": ["doc.pdf"]}"#,
    )
    .unwrap();
    let created = json(&run(
        dir,
        &[
            "manifest",
            "create-manifest",
            "--cli-input-json",
            "file://input.json",
            "--context",
            "file://context.txt",
        ],
    ));
    assert_eq!(created["Channel"], "slack");
    assert_eq!(created["Context"], "Slack #general");
    assert_eq!(created["Attachments"][0]["Path"], "doc.pdf");
}

#[test]
fn skeletons_use_pascal_case_keys() {
    let directory = tempfile::tempdir().unwrap();
    let input = json(&run(
        directory.path(),
        &["manifest", "seal-manifest", "--generate-cli-skeleton"],
    ));
    let keys: Vec<&String> = input.as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        ["ManifestFile", "ApprovedBy", "ApprovalNote", "ApprovedAt"]
    );
    assert_eq!(input["ApprovedBy"], "user");
    let output = json(&run(
        directory.path(),
        &[
            "manifest",
            "review-manifest",
            "--generate-cli-skeleton",
            "output",
        ],
    ));
    assert!(output["Body"].is_string());
    assert!(output["Attachments"][0]["Sha256"].is_string());
}

#[test]
fn query_and_text_output() {
    let directory = fixture();
    let dir = directory.path();
    json(&run(dir, CREATE));
    let output = run(
        dir,
        &with(
            &[
                "manifest",
                "check-manifest",
                "--query",
                "Attachments[0].Path",
                "--output",
                "text",
            ],
            MANIFEST,
        ),
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "doc.pdf\n");
}
