# outbound-manifest

メール、Slack/Discord、SNS、フォーム送信の「送信先・確定本文・添付」を単一 manifest にし、ユーザー承認後の変更を SHA-256 で検出する Rust CLI。送信機能は持たない。

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

本文ファイルは前半に前置きを置ける。`---` だけの行が最初に現れたところを区切りとし、その上に送信の文脈・出典・判断材料を Markdown で書き、その下に送信する文字列だけを置く。`check-manifest` / `review-manifest` / `seal-manifest` / `verify-manifest` は区切りより下の本文だけを対象にするため、承認後に前置きを書き足しても `verify-manifest` は通り、本文が変われば失敗する。`---` の行が無いファイルは全体が本文になる。

`check-manifest` は空本文、不正なメールアドレス、重複/空/欠落添付、添付 hash の変化、`TODO` / `TBD` / `FIXME` / `{{...}}` / `<insert...>` / `[要確認]` を拒否する。`seal-manifest` は本文ファイルと添付の hash を含む content hash を記録する。承認後に宛先・件名・本文・添付・context のいずれかが変わると `verify-manifest` が失敗するため、変更後は再度 review とユーザー承認が必要になる。

`seal-manifest` はユーザー承認を取得する機能ではない。エージェントはユーザーの明示承認なしに実行してはならない。

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
- 終了コードは 0 が成功、252 が文法・パラメータの誤り、254 が manifest 側の誤り (検査の失敗、承認後の変更、manifest・本文・添付の読み込み失敗)、255 がそれ以外の失敗。

manifest の JSON (`schema: 1`、snake_case のキー、`body_file`、`approval`) と content hash の計算は CLI の文法から独立している。

## Development

```sh
nix develop -c cargo fmt --check
nix develop -c cargo clippy --all-targets -- -D warnings
nix develop -c cargo test
```
