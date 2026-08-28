# rwinSCP

A single-binary desktop client for **SSH, SFTP, SCP, FTP, and S3**, built
with `egui`/`eframe`.

## Architecture

```
src/
  main.rs           entry point, window setup
  theme.rs          design tokens (palette, radii, spacing) + global style
  connection.rs     Protocol / Auth / ConnectionProfile
  app.rs            application state, Action reducer, worker polling
  backend/
    mod.rs          Command/Event contract + worker spawning
    ssh_sftp_scp.rs libssh2-backed SSH, SFTP, SCP
    ftp.rs          suppaftp-backed FTP
    s3.rs           aws-sdk-s3-backed object storage
  icon.rs           named glyphs from the bundled Phosphor font
  ui/
    mod.rs          root layout
    rail.rs         left icon rail
    dashboard.rs    Hosts page: search, recents, actions, grid
    host_card.rs    one host tile
    editor.rs       host form
    explorer.rs     breadcrumbs, toolbar, file table
    log_panel.rs    activity log
    widgets.rs      shared primitives
```
