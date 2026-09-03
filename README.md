<p align="center">
  <img src="assets/icon.png" alt="rwinSCP logo" width="235">
</p>
<h1 align="center">
  <b>rwinSCP</b>
</h1>

<p align="center">
    <img alt="Rust" src="https://img.shields.io/badge/Rust-stable%20(MSRV)-orange?logo=rust">
    <a href="https://github.com/Rinny-Inc/rwinSCP/actions/workflows/test.yml"><img alt="Build" src="https://github.com/Rinny-Inc/rwinSCP/actions/workflows/test.yml/badge.svg"></a>
    <a href="https://github.com/Rinny-Inc/rwinSCP/releases">
        <img src="https://img.shields.io/github/v/release/Rinny-Inc/rwinSCP" alt="Release"/>
        <img src="https://img.shields.io/github/downloads/Rinny-Inc/rwinSCP/total" alt="Downloads"/>
    </a>
    <img src="https://img.shields.io/badge/Platform-macOS%20%7C%20Windows%20%7C%20Linux-blue" alt="Platform"/>
    <a href="https://discord.com/invite/B2BgjwDX8m"><img alt="Discord" src="https://img.shields.io/discord/1352833901860487299?label=Discord&logo=discord"></a>
</p>

A single-binary desktop client for **SSH, SFTP, SCP, FTP, and S3**, built
with `egui`/`eframe`.

## Architecture

```
src/
  main.rs           entry point, window setup
  theme.rs          design tokens (palette, radii, spacing) + global style
  connection.rs     Protocol / Auth / ConnectionProfile
  app.rs            application state, Action reducer, worker polling
  store.rs          credentials management
  backend/
    mod.rs          Command/Event contract + worker spawning
    ssh_sftp_scp.rs libssh2-backed SSH, SFTP, SCP
    ftp.rs          suppaftp-backed FTP
    s3.rs           aws-sdk-s3-backed object storage
  icon.rs           named glyphs from the bundled Phosphor font
  ui/
    mod.rs          root layout
    tabs.rs         session tab strip
    terminal.rs     interactive shell (raw keystrokes to a PTY)
    rail.rs         left icon rail
    dashboard.rs    Hosts page: search, recents, actions, grid
    host_card.rs    one host tile
    history.rs      downloads/uploads history
    editor.rs       host form
    explorer.rs     breadcrumbs, toolbar, file table
    log_panel.rs    activity log
    widgets.rs      shared primitives
```
