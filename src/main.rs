use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

type Result<T> = std::result::Result<T, String>;

#[derive(Parser)]
#[command(
    name = "outbound-manifest",
    version,
    about = "Review, seal, and verify external communications"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Create(CreateArgs),
    Check(ManifestArgs),
    Review(ManifestArgs),
    Seal(SealArgs),
    Verify(ManifestArgs),
}

#[derive(Clone, Debug, Deserialize, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
#[clap(rename_all = "snake_case")]
enum Channel {
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

#[derive(Args)]
struct CreateArgs {
    #[arg(long)]
    manifest: PathBuf,
    #[arg(long, value_enum)]
    channel: Channel,
    #[arg(long)]
    context: String,
    #[arg(long)]
    to: Vec<String>,
    #[arg(long)]
    cc: Vec<String>,
    #[arg(long)]
    bcc: Vec<String>,
    #[arg(long)]
    subject: Option<String>,
    #[arg(long)]
    body_file: PathBuf,
    #[arg(long)]
    attachment: Vec<PathBuf>,
}

#[derive(Args)]
struct ManifestArgs {
    #[arg(long)]
    manifest: PathBuf,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct SealArgs {
    #[arg(long)]
    manifest: PathBuf,
    #[arg(long, default_value = "user")]
    by: String,
    #[arg(long)]
    approval_note: String,
    #[arg(long)]
    approved_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Manifest {
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
struct CheckReport {
    manifest: PathBuf,
    channel: String,
    context: String,
    recipients: usize,
    body_bytes: u64,
    body_sha256: String,
    attachments: Vec<AttachmentReport>,
    content_sha256: String,
    sealed: bool,
}

#[derive(Debug, Serialize)]
struct AttachmentReport {
    path: PathBuf,
    bytes: u64,
    sha256: String,
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

struct LoadedManifest {
    path: PathBuf,
    base: PathBuf,
    manifest: Manifest,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    match Cli::parse().command {
        Commands::Create(args) => create(args)?,
        Commands::Check(args) => {
            let loaded = load(&args.manifest)?;
            let report = check(&loaded)?;
            print_report(&report, args.json)?;
        }
        Commands::Review(args) => review(&args.manifest)?,
        Commands::Seal(args) => seal(args)?,
        Commands::Verify(args) => {
            let loaded = load(&args.manifest)?;
            let report = verify(&loaded)?;
            print_report(&report, args.json)?;
        }
    }
    Ok(())
}

fn create(args: CreateArgs) -> Result<()> {
    if args.manifest.exists() {
        return Err(format!("{} already exists", args.manifest.display()));
    }
    if let Some(parent) = args.manifest.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let base = args.manifest.parent().unwrap_or_else(|| Path::new("."));
    let mut attachments = Vec::new();
    for path in args.attachment {
        attachments.push(Attachment {
            sha256: sha256(&resolve(base, &path))?,
            path,
        });
    }
    let manifest = Manifest {
        schema: 1,
        channel: args.channel,
        context: args.context,
        to: args.to,
        cc: args.cc,
        bcc: args.bcc,
        subject: args.subject,
        body_file: args.body_file,
        attachments,
        allow_placeholders: Vec::new(),
        approval: None,
    };
    atomic_write_json(&args.manifest, &manifest)?;
    let report = check(&load(&args.manifest)?)?;
    println!(
        "created {}; review content sha256:{}",
        args.manifest.display(),
        report.content_sha256
    );
    Ok(())
}

fn load(path: &Path) -> Result<LoadedManifest> {
    let raw = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let manifest =
        serde_json::from_str(&raw).map_err(|error| format!("{}: {error}", path.display()))?;
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
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(hex::encode(Sha256::digest(bytes)))
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

fn check(loaded: &LoadedManifest) -> Result<CheckReport> {
    let manifest = &loaded.manifest;
    if manifest.schema != 1 {
        return Err(format!(
            "unsupported manifest schema {}; expected 1",
            manifest.schema
        ));
    }
    if manifest.context.trim().is_empty() {
        return Err(
            "context must identify the exact thread, channel, account, or form".to_string(),
        );
    }
    if matches!(manifest.channel, Channel::Email) {
        if manifest.to.is_empty() {
            return Err("email manifest requires at least one recipient in `to`".to_string());
        }
        if manifest.subject.as_deref().unwrap_or("").trim().is_empty() {
            return Err("email manifest requires a non-empty subject".to_string());
        }
        for address in manifest.to.iter().chain(&manifest.cc).chain(&manifest.bcc) {
            if !basic_email_valid(address) {
                return Err(format!("invalid email address: {address}"));
            }
        }
    }
    let body_path = resolve(&loaded.base, &manifest.body_file);
    let body = fs::read_to_string(&body_path)
        .map_err(|error| format!("{}: {error}", body_path.display()))?;
    if body.trim().is_empty() {
        return Err("body file is empty".to_string());
    }
    let mut hits = placeholder_hits(&body, &manifest.allow_placeholders);
    if let Some(subject) = &manifest.subject {
        hits.extend(placeholder_hits(subject, &manifest.allow_placeholders));
    }
    if !hits.is_empty() {
        hits.sort_unstable();
        hits.dedup();
        return Err(format!("unresolved placeholder(s): {}", hits.join(", ")));
    }

    let body_metadata = fs::metadata(&body_path).map_err(|error| error.to_string())?;
    let body_sha256 = sha256(&body_path)?;
    let mut seen = HashSet::new();
    let mut attachment_reports = Vec::new();
    for attachment in &manifest.attachments {
        let path = resolve(&loaded.base, &attachment.path);
        let canonical = path
            .canonicalize()
            .map_err(|error| format!("{}: {error}", path.display()))?;
        if !seen.insert(canonical) {
            return Err(format!(
                "duplicate attachment: {}",
                attachment.path.display()
            ));
        }
        let metadata =
            fs::metadata(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(format!(
                "attachment is not a non-empty file: {}",
                path.display()
            ));
        }
        let actual = sha256(&path)?;
        if actual != attachment.sha256 {
            return Err(format!(
                "attachment sha256 mismatch for {}: manifest {}, actual {actual}",
                attachment.path.display(),
                attachment.sha256
            ));
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
    let encoded = serde_json::to_vec(&canonical).map_err(|error| error.to_string())?;
    let content_sha256 = hex::encode(Sha256::digest(encoded));
    Ok(CheckReport {
        manifest: loaded.path.clone(),
        channel: manifest.channel.as_str().to_string(),
        context: manifest.context.clone(),
        recipients: manifest.to.len() + manifest.cc.len() + manifest.bcc.len(),
        body_bytes: body_metadata.len(),
        body_sha256,
        attachments: attachment_reports,
        content_sha256,
        sealed: manifest.approval.is_some(),
    })
}

fn review(path: &Path) -> Result<()> {
    let loaded = load(path)?;
    let report = check(&loaded)?;
    let manifest = &loaded.manifest;
    let body_path = resolve(&loaded.base, &manifest.body_file);
    let body = fs::read_to_string(&body_path).map_err(|error| error.to_string())?;
    println!("# External action review");
    println!();
    println!("- Manifest: {}", path.display());
    println!("- Channel: {}", manifest.channel.as_str());
    println!("- Context: {}", manifest.context);
    println!("- To: {}", manifest.to.join(", "));
    println!("- Cc: {}", manifest.cc.join(", "));
    println!("- Bcc: {}", manifest.bcc.join(", "));
    println!("- Subject: {}", manifest.subject.as_deref().unwrap_or(""));
    println!("- Content SHA-256: {}", report.content_sha256);
    println!();
    println!("## Body ({})", manifest.body_file.display());
    println!();
    print!("{body}");
    if !body.ends_with('\n') {
        println!();
    }
    println!();
    println!("## Attachments");
    println!();
    if report.attachments.is_empty() {
        println!("- (none)");
    } else {
        for attachment in report.attachments {
            println!(
                "- {} — {} bytes — sha256:{}",
                attachment.path.display(),
                attachment.bytes,
                attachment.sha256
            );
        }
    }
    Ok(())
}

fn seal(args: SealArgs) -> Result<()> {
    if args.by.trim().is_empty() || args.approval_note.trim().is_empty() {
        return Err("--by and --approval-note must be non-empty".to_string());
    }
    let mut loaded = load(&args.manifest)?;
    let report = check(&loaded)?;
    let approved_at = match args.approved_at {
        Some(value) => DateTime::parse_from_rfc3339(&value)
            .map_err(|_| "--approved-at must be RFC 3339".to_string())?
            .with_timezone(&Utc),
        None => Utc::now(),
    };
    loaded.manifest.approval = Some(Approval {
        by: args.by,
        approved_at,
        note: args.approval_note,
        content_sha256: report.content_sha256.clone(),
    });
    atomic_write_json(&args.manifest, &loaded.manifest)?;
    println!(
        "sealed {} at content sha256:{}",
        args.manifest.display(),
        report.content_sha256
    );
    Ok(())
}

fn verify(loaded: &LoadedManifest) -> Result<CheckReport> {
    let report = check(loaded)?;
    let approval = loaded
        .manifest
        .approval
        .as_ref()
        .ok_or_else(|| "manifest is not sealed by an explicit user approval".to_string())?;
    if approval.content_sha256 != report.content_sha256 {
        return Err(format!(
            "approved content changed: sealed {}, current {}",
            approval.content_sha256, report.content_sha256
        ));
    }
    Ok(report)
}

fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent", path.display()))?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut encoded = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    encoded.push(b'\n');
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    temporary
        .write_all(&encoded)
        .map_err(|error| error.to_string())?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    temporary
        .persist(path)
        .map_err(|error| error.error.to_string())?;
    Ok(())
}

fn print_report(report: &CheckReport, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(report).map_err(|error| error.to_string())?
        );
    } else {
        println!(
            "{}: {} recipient(s), {} attachment(s), sealed={}, content sha256:{}",
            report.channel,
            report.recipients,
            report.attachments.len(),
            report.sealed,
            report.content_sha256
        );
    }
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

    #[test]
    fn seal_and_verify_detect_body_changes() {
        let (directory, path) = fixture();
        let mut loaded = load(&path).unwrap();
        let report = check(&loaded).unwrap();
        loaded.manifest.approval = Some(Approval {
            by: "user".to_string(),
            approved_at: Utc::now(),
            note: "approved in chat".to_string(),
            content_sha256: report.content_sha256,
        });
        atomic_write_json(&path, &loaded.manifest).unwrap();
        verify(&load(&path).unwrap()).unwrap();
        fs::write(directory.path().join("body.md"), "changed\n").unwrap();
        assert!(verify(&load(&path).unwrap())
            .unwrap_err()
            .contains("approved content changed"));
    }

    #[test]
    fn unresolved_placeholder_is_rejected() {
        let (directory, path) = fixture();
        fs::write(directory.path().join("body.md"), "Hello TODO\n").unwrap();
        assert!(check(&load(&path).unwrap())
            .unwrap_err()
            .contains("unresolved placeholder"));
    }

    #[test]
    fn changed_attachment_is_rejected_before_sealing() {
        let (directory, path) = fixture();
        fs::write(directory.path().join("contract.pdf"), b"different").unwrap();
        assert!(check(&load(&path).unwrap())
            .unwrap_err()
            .contains("attachment sha256 mismatch"));
    }
}
