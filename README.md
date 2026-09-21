# outbound-manifest

メール、Slack/Discord、SNS、フォーム送信の「送信先・確定本文・添付」を単一 manifest にし、ユーザー承認後の変更を SHA-256 で検出する Rust CLI。送信機能は持たない。

## Workflow

```sh
outbound-manifest create \
  --manifest .claude-dev/task/outbound.json \
  --channel email \
  --context 'Gmail sender@example.com / thread subject' \
  --to person@example.com \
  --cc team@example.com \
  --subject 'Documents' \
  --body-file body.md \
  --attachment document.pdf

outbound-manifest check --manifest .claude-dev/task/outbound.json
outbound-manifest review --manifest .claude-dev/task/outbound.json

# ユーザーが review の宛先・本文・添付を明示承認した後だけ実行する
outbound-manifest seal \
  --manifest .claude-dev/task/outbound.json \
  --by user \
  --approval-note '2026-08-28 chat: 送って'

# 外部サービスの送信ボタンを押す直前に必須
outbound-manifest verify --manifest .claude-dev/task/outbound.json
```

本文ファイルは先頭に `---` で囲んだ frontmatter を置ける。frontmatter には送信の文脈・出典・判断材料を書き、`---` の下に送信する文字列だけを置く。`check` / `review` / `seal` / `verify` は frontmatter を除いた本文だけを対象にするため、承認後に frontmatter を書き足しても `verify` は通り、本文が変われば失敗する。frontmatter を開いて `---` の行で閉じないファイルは `check` が拒否する。

`check` は空本文、不正なメールアドレス、重複/空/欠落添付、添付 hash の変化、`TODO` / `TBD` / `FIXME` / `{{...}}` / `<insert...>` / `[要確認]` を拒否する。`seal` は本文ファイルと添付の hash を含む content hash を記録する。承認後に宛先・件名・本文・添付・context のいずれかが変わると `verify` が失敗するため、変更後は再度 review とユーザー承認が必要になる。

`seal` はユーザー承認を取得する機能ではない。エージェントはユーザーの明示承認なしに実行してはならない。

## Development

```sh
nix develop -c cargo fmt --check
nix develop -c cargo clippy --all-targets -- -D warnings
nix develop -c cargo test
```
