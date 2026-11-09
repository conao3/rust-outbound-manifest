<p align="center">
  <img src="docs/assets/banner.png" alt="outbound-manifest: approval-sealed manifests for validating outbound messages and attachments before sending" width="880">
</p>

<p align="center"><strong>Seal the message you approved, and catch any change to it before you send</strong></p>

<p align="center">
  <a href="Cargo.toml"><img src="https://img.shields.io/badge/version-0.2.0-blue" alt="Version 0.2.0"></a>
  <a href="Cargo.toml"><img src="https://img.shields.io/badge/license-MIT-green" alt="License: MIT"></a>
  <a href="Cargo.toml"><img src="https://img.shields.io/badge/Rust-2021_edition-orange?logo=rust" alt="Rust 2021 edition"></a>
  <a href="flake.nix"><img src="https://img.shields.io/badge/Nix-flake-5277C3?logo=nixos&logoColor=white" alt="Nix flake"></a>
  <a href="https://github.com/conao3/rust-outbound-manifest/stargazers"><img src="https://img.shields.io/github/stars/conao3/rust-outbound-manifest?style=flat" alt="GitHub stars"></a>
</p>

<p align="center">
  <a href="#install">Install</a> ·
  <a href="#workflow">Workflow</a> ·
  <a href="#checks">Checks</a> ·
  <a href="#commands">Commands</a> ·
  <a href="#development">Development</a>
</p>

# outbound-manifest

メール、Slack/Discord、SNS、フォーム送信の「送信先・確定本文・添付」を単一 manifest にし、ユーザー承認後の変更を SHA-256 で検出する Rust CLI。送信機能は持たない。

- 送信前に、宛先・件名・本文・添付・context を 1 つの JSON manifest に固定する。
- `review-manifest --output text` が承認用の Markdown を出し、ユーザーはそれを見て承認する。
- `seal-manifest` が承認時の content hash を記録し、`verify-manifest` が送信直前に変更を検出する。
- エージェントに送信を任せるとき、ユーザーが見て承認した内容と実際に送る内容が同じであることを確かめられる。

## Install

リポジトリの flake が CLI の package を提供する。

```sh
nix profile install github:conao3/rust-outbound-manifest
```

Cargo で入れる場合は次のとおり。

```sh
cargo install --git https://github.com/conao3/rust-outbound-manifest
```

## Workflow

```sh
outbound-manifest manifest create-manifest \
  --manifest-file .claude-dev/task/outbound.json \
  --channel email \
  --context 'Gmail sender@example.com / thread subject' \
  --to person@example.com other@example.com \
  --cc team@example.com \
  --subject 'Documents' \
  --body-file body.md \
  --attachments document.pdf

outbound-manifest manifest check-manifest --manifest-file .claude-dev/task/outbound.json
outbound-manifest manifest review-manifest --manifest-file .claude-dev/task/outbound.json --output text

# ユーザーが review の宛先・本文・添付を明示承認した後だけ実行する
outbound-manifest manifest seal-manifest \
  --manifest-file .claude-dev/task/outbound.json \
  --approved-by user \
  --approval-note '2026-08-28 chat: 送って'

# 外部サービスの送信ボタンを押す直前に必須
outbound-manifest manifest verify-manifest --manifest-file .claude-dev/task/outbound.json
```

### 本文ファイルの前置き

本文ファイルは前半に前置きを置ける。`---` だけの行が最初に現れたところを区切りとし、その上に送信の文脈・出典・判断材料を Markdown で書き、その下に送信する文字列だけを置く。`check-manifest` / `review-manifest` / `seal-manifest` / `verify-manifest` は区切りより下の本文だけを対象にするため、承認後に前置きを書き足しても `verify-manifest` は通り、本文が変われば失敗する。`---` の行が無いファイルは全体が本文になる。

```markdown
- 宛先: person@example.com
- 出典: 先方からのメール (thread subject)

---
送信する本文はここから下に書く。
```

## Checks

`check-manifest` は空本文、不正なメールアドレス、重複/空/欠落添付、添付 hash の変化、`TODO` / `TBD` / `FIXME` / `{{...}}` / `<insert...>` / `[要確認]` を拒否する。`seal-manifest` は本文ファイルと添付の hash を含む content hash を記録する。承認後に宛先・件名・本文・添付・context のいずれかが変わると `verify-manifest` が失敗するため、変更後は再度 review とユーザー承認が必要になる。

> [!IMPORTANT]
> `seal-manifest` はユーザー承認を取得する機能ではない。エージェントはユーザーの明示承認なしに実行してはならない。

## Commands

AWS CLI と同じ文法 `outbound-manifest [global options] <command> <subcommand> [--parameter value ...]` で入力を受ける。

| subcommand | parameters | output |
|---|---|---|
| `manifest create-manifest` | `--manifest-file` `--channel email\|slack\|discord\|social\|form` `--context` `--body-file` (必須)、`--to` `--cc` `--bcc` `--attachments` (空白区切りのリスト)、`--subject` (email では必須) | check の結果 |
| `manifest check-manifest` | `--manifest-file` | `ManifestFile` `Channel` `Context` `Recipients` `BodyBytes` `BodySha256` `Attachments` `ContentSha256` `Sealed` |
| `manifest review-manifest` | `--manifest-file` | 宛先・件名・`Body`・添付・`ContentSha256`。`--output text` で承認用の Markdown を出す |
| `manifest seal-manifest` | `--manifest-file` `--approval-note` (必須)、`--approved-by` (既定 `user`)、`--approved-at` (RFC 3339、既定は現在時刻) | `ManifestFile` `ApprovedBy` `ApprovedAt` `ApprovalNote` `ContentSha256` |
| `manifest verify-manifest` | `--manifest-file` | check の結果。承認時の content hash と一致しなければ失敗する |

- `--body-file` と `--attachments` のパスは manifest のディレクトリからの相対で解決する。
- 値の先頭が `file://` ならファイルの中身を値として読む (例: `--context file://context.txt`)。パスを取るパラメータにはパスをそのまま渡す。
- グローバルオプションは `--output json|text|table` (既定 `json`)、`--debug`、`--version`。出力のキーは PascalCase。
- `outbound-manifest help`、`outbound-manifest manifest help`、`outbound-manifest manifest <subcommand> help` で各階層のヘルプを出す。
- content hash だけを取り出すときは `| jq -r .ContentSha256` を使う。

### 終了コード

| code | 意味 |
|---|---|
| 0 | 成功 |
| 252 | 文法・パラメータの誤り |
| 254 | manifest 側の誤り (検査の失敗、承認後の変更、manifest・本文・添付の読み込み失敗) |
| 255 | それ以外の失敗 |

manifest の JSON (`schema: 1`、snake_case のキー、`body_file`、`approval`) と content hash の計算は CLI の文法から独立している。

## Development

リポジトリの flake が devShell と formatter を提供する。検査は次の 3 つ。

```sh
nix develop -c cargo fmt --check
nix develop -c cargo clippy --all-targets -- -D warnings
nix develop -c cargo test
```

`tests/cli.rs` がバイナリを end-to-end で動かす。
