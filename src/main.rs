mod error;
mod grammar;
mod manifest;
mod output;

use std::path::PathBuf;

use clap::{Args, CommandFactory, Parser, Subcommand};
use serde::Serialize;
use serde_json::Value;

use error::{other, Result};
use manifest::{Channel, NewManifest};
use output::{OutputFormat, Response};

#[derive(Parser)]
#[command(
    name = "outbound-manifest",
    version,
    about = "Review, seal, and verify external communications"
)]
struct Cli {
    /// Output format
    #[arg(long, global = true, help_heading = "Global Options", value_enum, default_value_t = OutputFormat::Json)]
    output: OutputFormat,
    /// Print debug logs to stderr
    #[arg(long, global = true, help_heading = "Global Options")]
    debug: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create, check, review, seal, and verify outbound manifests
    #[command(subcommand)]
    Manifest(ManifestCommand),
}

#[derive(Subcommand)]
enum ManifestCommand {
    /// Write a new manifest that fixes the channel, context, recipients, subject, body file, and attachments
    #[command(name = "create-manifest")]
    Create(CreateManifestArgs),
    /// Check placeholders, recipients, and the body and attachment hashes
    #[command(name = "check-manifest")]
    Check(ManifestFileArgs),
    /// Show the recipients, body, attachments, and content hash for approval (use --output text to read it)
    #[command(name = "review-manifest")]
    Review(ManifestFileArgs),
    /// Record the user's explicit approval of the reviewed content hash. Run only after the user approved this exact content
    #[command(name = "seal-manifest")]
    Seal(SealManifestArgs),
    /// Fail unless the current content matches the sealed approval. Run right before sending
    #[command(name = "verify-manifest")]
    Verify(ManifestFileArgs),
}

#[derive(Args)]
struct CreateManifestArgs {
    /// Path of the manifest JSON to create
    #[arg(long)]
    manifest_file: PathBuf,
    /// Channel of the outbound message
    #[arg(long, value_enum)]
    channel: Channel,
    /// The exact thread, channel, account, or form the message goes to
    #[arg(long)]
    context: String,
    /// Recipients (space-separated)
    #[arg(long, num_args = 1..)]
    to: Vec<String>,
    /// Cc recipients (space-separated)
    #[arg(long, num_args = 1..)]
    cc: Vec<String>,
    /// Bcc recipients (space-separated)
    #[arg(long, num_args = 1..)]
    bcc: Vec<String>,
    /// Subject (required for email)
    #[arg(long)]
    subject: Option<String>,
    /// Path of the body file, relative to the manifest directory. Only the text below the first `---` line is the body
    #[arg(long)]
    body_file: PathBuf,
    /// Attachment paths relative to the manifest directory (space-separated)
    #[arg(long, num_args = 1..)]
    attachments: Vec<PathBuf>,
}

#[derive(Args)]
struct ManifestFileArgs {
    /// Path of the manifest JSON
    #[arg(long)]
    manifest_file: PathBuf,
}

#[derive(Args)]
struct SealManifestArgs {
    /// Path of the manifest JSON
    #[arg(long)]
    manifest_file: PathBuf,
    /// Who approved the content
    #[arg(long, default_value = "user")]
    approved_by: String,
    /// Where and how the approval was given
    #[arg(long)]
    approval_note: String,
    /// Approval time in RFC 3339 (defaults to now)
    #[arg(long)]
    approved_at: Option<String>,
}

fn main() {
    let code = match run() {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("error: {error}");
            error.exit_code()
        }
    };
    std::process::exit(code);
}

fn to_value<T: Serialize>(value: &T) -> Result<Value> {
    serde_json::to_value(value).map_err(|error| other(error.to_string()))
}

fn response<T: Serialize>(report: &T, text: Option<String>) -> Result<Response> {
    Ok(Response {
        value: to_value(report)?,
        text,
    })
}

fn run() -> Result<()> {
    let mut root = Cli::command();
    root.build();
    let argv = grammar::prepare(&root, std::env::args_os().collect())?;
    if argv.iter().any(|token| token == "--debug") {
        eprintln!("debug: argv {:?}", &argv[1..]);
    }

    let cli = match Cli::try_parse_from(&argv) {
        Ok(cli) => cli,
        Err(error) => {
            let code = if error.use_stderr() { 252 } else { 0 };
            let _ = error.print();
            std::process::exit(code);
        }
    };

    let Command::Manifest(command) = cli.command;
    let (name, response) = match command {
        ManifestCommand::Create(args) => {
            let report = manifest::create(
                &args.manifest_file,
                NewManifest {
                    channel: args.channel,
                    context: args.context,
                    to: args.to,
                    cc: args.cc,
                    bcc: args.bcc,
                    subject: args.subject,
                    body_file: args.body_file,
                    attachments: args.attachments,
                },
            )?;
            ("create-manifest", response(&report, None)?)
        }
        ManifestCommand::Check(args) => {
            let report = manifest::check(&manifest::load(&args.manifest_file)?)?;
            ("check-manifest", response(&report, None)?)
        }
        ManifestCommand::Review(args) => {
            let report = manifest::review(&manifest::load(&args.manifest_file)?)?;
            let text = report.to_markdown();
            ("review-manifest", response(&report, Some(text))?)
        }
        ManifestCommand::Seal(args) => {
            let report = manifest::seal(
                &args.manifest_file,
                args.approved_by,
                args.approval_note,
                args.approved_at,
            )?;
            ("seal-manifest", response(&report, None)?)
        }
        ManifestCommand::Verify(args) => {
            let report = manifest::verify(&manifest::load(&args.manifest_file)?)?;
            ("verify-manifest", response(&report, None)?)
        }
    };
    print!(
        "{}",
        output::render(&grammar::pascal_case(name), response, cli.output)?
    );
    Ok(())
}
