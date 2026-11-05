use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{other, parameter, target, Result};

#[derive(Clone, Debug, Deserialize, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
#[clap(rename_all = "snake_case")]
pub enum Channel {
    Email,
    Slack,
    Discord,
    Social,
    Form,
}

impl Channel {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::Slack => "slack",
            Self::Discord => "discord",
            Self::Social => "social",
            Self::Form => "form",
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Manifest {
    schema: u32,
    channel: Channel,
    context: String,
    #[serde(default)]
    to: Vec<String>,
    #[serde(default)]
    cc: Vec<String>,
    #[serde(default)]
    bcc: Vec<String>,
    #[serde(default)]
    subject: Option<String>,
    body_file: PathBuf,
    #[serde(default)]
    attachments: Vec<Attachment>,
    #[serde(default)]
    allow_placeholders: Vec<String>,
    #[serde(default)]
    approval: Option<Approval>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Attachment {
    path: PathBuf,
    sha256: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct Approval {
    by: String,
    approved_at: DateTime<Utc>,
    note: String,
    content_sha256: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct CheckReport {
    pub manifest_file: PathBuf,
    pub channel: String,
    pub context: String,
    pub recipients: usize,
    pub body_bytes: u64,
    pub body_sha256: String,
    pub attachments: Vec<AttachmentReport>,
    pub content_sha256: String,
    pub sealed: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct AttachmentReport {
    pub path: PathBuf,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ReviewReport {
    pub manifest_file: PathBuf,
    pub channel: String,
    pub context: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: Option<String>,
    pub body_file: PathBuf,
    pub body: String,
    pub attachments: Vec<AttachmentReport>,
    pub content_sha256: String,
    pub sealed: bool,
}

impl ReviewReport {
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("# External action review\n\n");
        out.push_str(&format!("- Manifest: {}\n", self.manifest_file.display()));
        out.push_str(&format!("- Channel: {}\n", self.channel));
        out.push_str(&format!("- Context: {}\n", self.context));
        out.push_str(&format!("- To: {}\n", self.to.join(", ")));
        out.push_str(&format!("- Cc: {}\n", self.cc.join(", ")));
        out.push_str(&format!("- Bcc: {}\n", self.bcc.join(", ")));
        out.push_str(&format!(
            "- Subject: {}\n",
            self.subject.as_deref().unwrap_or("")
        ));
        out.push_str(&format!("- Content SHA-256: {}\n", self.content_sha256));
        out.push_str(&format!("\n## Body ({})\n\n", self.body_file.display()));
        out.push_str(&self.body);
        if !self.body.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("\n## Attachments\n\n");
        if self.attachments.is_empty() {
            out.push_str("- (none)\n");
        }
        for attachment in &self.attachments {
            out.push_str(&format!(
                "- {} — {} bytes — sha256:{}\n",
                attachment.path.display(),
                attachment.bytes,
                attachment.sha256
            ));
        }
        out
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct SealReport {
    pub manifest_file: PathBuf,
    pub approved_by: String,
    pub approved_at: String,
    pub approval_note: String,
    pub content_sha256: String,
}

#[derive(Serialize)]
struct CanonicalContent<'a> {
    schema: u32,
    channel: &'a str,
    context: &'a str,
    to: &'a [String],
    cc: &'a [String],
    bcc: &'a [String],
    subject: &'a Option<String>,
    body_file: &'a Path,
    body_sha256: &'a str,
    attachments: Vec<CanonicalAttachment<'a>>,
}

#[derive(Serialize)]
struct CanonicalAttachment<'a> {
    path: &'a Path,
    sha256: &'a str,
}

struct BodyDocument<'a> {
    preamble: Option<&'a str>,
    body: &'a str,
}

pub struct LoadedManifest {
    path: PathBuf,
    base: PathBuf,
    manifest: Manifest,
}

pub struct NewManifest {
    pub channel: Channel,
    pub context: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: Option<String>,
    pub body_file: PathBuf,
    pub attachments: Vec<PathBuf>,
}

pub fn create(path: &Path, input: NewManifest) -> Result<CheckReport> {
    if path.exists() {
        return Err(target(format!("{} already exists", path.display())));
    }
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let mut attachments = Vec::new();
    for attachment in input.attachments {
        attachments.push(Attachment {
            sha256: sha256(&resolve(base, &attachment))?,
            path: attachment,
        });
    }
    let manifest = Manifest {
        schema: 1,
        channel: input.channel,
        context: input.context,
        to: input.to,
        cc: input.cc,
        bcc: input.bcc,
        subject: input.subject,
        body_file: input.body_file,
        attachments,
        allow_placeholders: Vec::new(),
        approval: None,
    };
    atomic_write_json(path, &manifest)?;
    check(&load(path)?)
}

pub fn load(path: &Path) -> Result<LoadedManifest> {
    let raw =
        fs::read_to_string(path).map_err(|error| target(format!("{}: {error}", path.display())))?;
    let manifest = serde_json::from_str(&raw)
        .map_err(|error| target(format!("{}: {error}", path.display())))?;
    Ok(LoadedManifest {
        path: path.to_path_buf(),
        base: path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf(),
        manifest,
    })
}

fn resolve(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn sha256(path: &Path) -> Result<String> {
    let bytes = fs::read(path).map_err(|error| target(format!("{}: {error}", path.display())))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn split_body(raw: &str) -> BodyDocument<'_> {
    let mut offset = 0;
    while offset <= raw.len() {
        let line_end = raw[offset..]
            .find('\n')
            .map_or(raw.len(), |index| offset + index);
        if raw[offset..line_end].trim_end_matches('\r') == "---" {
            return BodyDocument {
                preamble: Some(&raw[..offset]),
                body: raw[line_end..].trim_start_matches(['\n', '\r']),
            };
        }
        offset = line_end + 1;
    }
    BodyDocument {
        preamble: None,
        body: raw,
    }
}

fn read_body(loaded: &LoadedManifest) -> Result<String> {
    let body_path = resolve(&loaded.base, &loaded.manifest.body_file);
    let raw = fs::read_to_string(&body_path)
        .map_err(|error| target(format!("{}: {error}", body_path.display())))?;
    let document = split_body(&raw);
    if document.body.trim().is_empty() {
        return Err(target(match document.preamble {
            Some(_) => "body file has no content below the `---` separator",
            None => "body file is empty",
        }));
    }
    Ok(document.body.to_string())
}

fn basic_email_valid(address: &str) -> bool {
    let mut parts = address.split('@');
    matches!((parts.next(), parts.next(), parts.next()), (Some(local), Some(domain), None) if !local.is_empty() && domain.contains('.') && !address.chars().any(char::is_whitespace))
}

fn placeholder_hits<'a>(value: &'a str, allow: &[String]) -> Vec<&'a str> {
    const PATTERNS: [&str; 7] = ["TODO", "TBD", "FIXME", "{{", "}}", "<insert", "[要確認]"];
    PATTERNS
        .into_iter()
        .filter(|pattern| {
            value.contains(pattern) && !allow.iter().any(|allowed| allowed == pattern)
        })
        .collect()
}

pub fn check(loaded: &LoadedManifest) -> Result<CheckReport> {
    let manifest = &loaded.manifest;
    if manifest.schema != 1 {
        return Err(target(format!(
            "unsupported manifest schema {}; expected 1",
            manifest.schema
        )));
    }
    if manifest.context.trim().is_empty() {
        return Err(target(
            "context must identify the exact thread, channel, account, or form",
        ));
    }
    if matches!(manifest.channel, Channel::Email) {
        if manifest.to.is_empty() {
            return Err(target(
                "email manifest requires at least one recipient in `to`",
            ));
        }
        if manifest.subject.as_deref().unwrap_or("").trim().is_empty() {
            return Err(target("email manifest requires a non-empty subject"));
        }
        for address in manifest.to.iter().chain(&manifest.cc).chain(&manifest.bcc) {
            if !basic_email_valid(address) {
                return Err(target(format!("invalid email address: {address}")));
            }
        }
    }
    let body = read_body(loaded)?;
    let mut hits = placeholder_hits(&body, &manifest.allow_placeholders);
    if let Some(subject) = &manifest.subject {
        hits.extend(placeholder_hits(subject, &manifest.allow_placeholders));
    }
    if !hits.is_empty() {
        hits.sort_unstable();
        hits.dedup();
        return Err(target(format!(
            "unresolved placeholder(s): {}",
            hits.join(", ")
        )));
    }

    let body_bytes = body.len() as u64;
    let body_sha256 = hex::encode(Sha256::digest(body.as_bytes()));
    let mut seen = HashSet::new();
    let mut attachment_reports = Vec::new();
    for attachment in &manifest.attachments {
        let path = resolve(&loaded.base, &attachment.path);
        let canonical = path
            .canonicalize()
            .map_err(|error| target(format!("{}: {error}", path.display())))?;
        if !seen.insert(canonical) {
            return Err(target(format!(
                "duplicate attachment: {}",
                attachment.path.display()
            )));
        }
        let metadata =
            fs::metadata(&path).map_err(|error| target(format!("{}: {error}", path.display())))?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(target(format!(
                "attachment is not a non-empty file: {}",
                path.display()
            )));
        }
        let actual = sha256(&path)?;
        if actual != attachment.sha256 {
            return Err(target(format!(
                "attachment sha256 mismatch for {}: manifest {}, actual {actual}",
                attachment.path.display(),
                attachment.sha256
            )));
        }
        attachment_reports.push(AttachmentReport {
            path: attachment.path.clone(),
            bytes: metadata.len(),
            sha256: actual,
        });
    }

    let canonical = CanonicalContent {
        schema: manifest.schema,
        channel: manifest.channel.as_str(),
        context: &manifest.context,
        to: &manifest.to,
        cc: &manifest.cc,
        bcc: &manifest.bcc,
        subject: &manifest.subject,
        body_file: &manifest.body_file,
        body_sha256: &body_sha256,
        attachments: manifest
            .attachments
            .iter()
            .map(|attachment| CanonicalAttachment {
                path: &attachment.path,
                sha256: &attachment.sha256,
            })
            .collect(),
    };
    let encoded = serde_json::to_vec(&canonical).map_err(|error| other(error.to_string()))?;
    let content_sha256 = hex::encode(Sha256::digest(encoded));
    Ok(CheckReport {
        manifest_file: loaded.path.clone(),
        channel: manifest.channel.as_str().to_string(),
        context: manifest.context.clone(),
        recipients: manifest.to.len() + manifest.cc.len() + manifest.bcc.len(),
        body_bytes,
        body_sha256,
        attachments: attachment_reports,
        content_sha256,
        sealed: manifest.approval.is_some(),
    })
}

pub fn review(loaded: &LoadedManifest) -> Result<ReviewReport> {
    let report = check(loaded)?;
    let manifest = &loaded.manifest;
    Ok(ReviewReport {
        manifest_file: report.manifest_file,
        channel: report.channel,
        context: report.context,
        to: manifest.to.clone(),
        cc: manifest.cc.clone(),
        bcc: manifest.bcc.clone(),
        subject: manifest.subject.clone(),
        body_file: manifest.body_file.clone(),
        body: read_body(loaded)?,
        attachments: report.attachments,
        content_sha256: report.content_sha256,
        sealed: report.sealed,
    })
}

pub fn seal(
    path: &Path,
    approved_by: String,
    approval_note: String,
    approved_at: Option<String>,
) -> Result<SealReport> {
    if approved_by.trim().is_empty() || approval_note.trim().is_empty() {
        return Err(parameter(
            "--approved-by and --approval-note must be non-empty",
        ));
    }
    let approved_at = match approved_at {
        Some(value) => DateTime::parse_from_rfc3339(&value)
            .map_err(|_| parameter("--approved-at must be RFC 3339"))?
            .with_timezone(&Utc),
        None => Utc::now(),
    };
    let mut loaded = load(path)?;
    let report = check(&loaded)?;
    loaded.manifest.approval = Some(Approval {
        by: approved_by.clone(),
        approved_at,
        note: approval_note.clone(),
        content_sha256: report.content_sha256.clone(),
    });
    atomic_write_json(path, &loaded.manifest)?;
    Ok(SealReport {
        manifest_file: path.to_path_buf(),
        approved_by,
        approved_at: approved_at.to_rfc3339(),
        approval_note,
        content_sha256: report.content_sha256,
    })
}

pub fn verify(loaded: &LoadedManifest) -> Result<CheckReport> {
    let report = check(loaded)?;
    let approval = loaded
        .manifest
        .approval
        .as_ref()
        .ok_or_else(|| target("manifest is not sealed by an explicit user approval"))?;
    if approval.content_sha256 != report.content_sha256 {
        return Err(target(format!(
            "approved content changed: sealed {}, current {}",
            approval.content_sha256, report.content_sha256
        )));
    }
    Ok(report)
}

fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    fs::create_dir_all(parent).map_err(|error| other(error.to_string()))?;
    let mut encoded = serde_json::to_vec_pretty(value).map_err(|error| other(error.to_string()))?;
    encoded.push(b'\n');
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|error| other(error.to_string()))?;
    temporary
        .write_all(&encoded)
        .map_err(|error| other(error.to_string()))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| other(error.to_string()))?;
    temporary
        .persist(path)
        .map_err(|error| other(error.error.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let body = directory.path().join("body.md");
        let attachment = directory.path().join("contract.pdf");
        let manifest = directory.path().join("outbound.json");
        fs::write(&body, "Hello,\n\nPlease see the attached file.\n").unwrap();
        fs::write(&attachment, b"%PDF-1.4 fixture").unwrap();
        let value = Manifest {
            schema: 1,
            channel: Channel::Email,
            context: "Gmail sender@example.com thread: onboarding".to_string(),
            to: vec!["person@example.com".to_string()],
            cc: vec![],
            bcc: vec![],
            subject: Some("Onboarding documents".to_string()),
            body_file: PathBuf::from("body.md"),
            attachments: vec![Attachment {
                path: PathBuf::from("contract.pdf"),
                sha256: sha256(&attachment).unwrap(),
            }],
            allow_placeholders: vec![],
            approval: None,
        };
        atomic_write_json(&manifest, &value).unwrap();
        (directory, manifest)
    }

    fn approve(path: &Path, content_sha256: String) {
        let mut loaded = load(path).unwrap();
        loaded.manifest.approval = Some(Approval {
            by: "user".to_string(),
            approved_at: Utc::now(),
            note: "approved in chat".to_string(),
            content_sha256,
        });
        atomic_write_json(path, &loaded.manifest).unwrap();
    }

    fn error_message<T>(result: Result<T>) -> String {
        match result {
            Ok(_) => panic!("expected an error"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn seal_and_verify_detect_body_changes() {
        let (directory, path) = fixture();
        let report = check(&load(&path).unwrap()).unwrap();
        approve(&path, report.content_sha256);
        verify(&load(&path).unwrap()).unwrap();
        fs::write(directory.path().join("body.md"), "changed\n").unwrap();
        assert!(error_message(verify(&load(&path).unwrap())).contains("approved content changed"));
    }

    #[test]
    fn unresolved_placeholder_is_rejected() {
        let (directory, path) = fixture();
        fs::write(directory.path().join("body.md"), "Hello TODO\n").unwrap();
        assert!(error_message(check(&load(&path).unwrap())).contains("unresolved placeholder"));
    }

    #[test]
    fn preamble_stays_out_of_the_body() {
        let (directory, path) = fixture();
        let body = directory.path().join("body.md");
        let preamble =
            "- 宛先: person@example.com\n- 判断材料: TODO 案 A と案 B のどちらにするか\n\n---\n\n";
        fs::write(&body, format!("{preamble}Hello,\n")).unwrap();
        let report = check(&load(&path).unwrap()).unwrap();
        assert_eq!(report.body_bytes, "Hello,\n".len() as u64);
        assert_eq!(
            report.body_sha256,
            hex::encode(Sha256::digest("Hello,\n".as_bytes()))
        );

        approve(&path, report.content_sha256);
        fs::write(
            &body,
            "- 宛先: person@example.com\n- 案 A で確定\n\n---\n\nHello,\n",
        )
        .unwrap();
        verify(&load(&path).unwrap()).unwrap();
        fs::write(
            &body,
            "- 宛先: person@example.com\n- 案 A で確定\n\n---\n\nHello!\n",
        )
        .unwrap();
        assert!(error_message(verify(&load(&path).unwrap())).contains("approved content changed"));
    }

    #[test]
    fn a_file_without_a_separator_is_all_body() {
        let (directory, path) = fixture();
        fs::write(directory.path().join("body.md"), "Hello,\n").unwrap();
        let report = check(&load(&path).unwrap()).unwrap();
        assert_eq!(report.body_bytes, "Hello,\n".len() as u64);
    }

    #[test]
    fn a_separator_without_a_body_is_rejected() {
        let (directory, path) = fixture();
        fs::write(directory.path().join("body.md"), "- 宛先: person\n\n---\n").unwrap();
        assert!(error_message(check(&load(&path).unwrap()))
            .contains("no content below the `---` separator"));
    }

    #[test]
    fn changed_attachment_is_rejected_before_sealing() {
        let (directory, path) = fixture();
        fs::write(directory.path().join("contract.pdf"), b"different").unwrap();
        assert!(error_message(check(&load(&path).unwrap())).contains("attachment sha256 mismatch"));
    }

    #[test]
    fn check_failures_are_target_errors() {
        let (directory, path) = fixture();
        fs::write(directory.path().join("body.md"), "Hello TODO\n").unwrap();
        let error = check(&load(&path).unwrap()).unwrap_err();
        assert_eq!(error.exit_code(), 254);
    }
}
