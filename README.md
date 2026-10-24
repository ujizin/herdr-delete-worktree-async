# herdr-delete-worktree-async

A [Herdr](https://herdr.dev) plugin that deletes a worktree checkout in the background.

## Why

Herdr's built-in "Delete worktree checkout..." shows a `removing…` modal until
`git worktree remove` returns. The client takes no input while it is up, and the workspace stays
open. On a large repo that lasts from several seconds to minutes. See
[herdrdev/herdr#1428](https://github.com/herdrdev/herdr/discussions/1428).

With this plugin you confirm the delete, the workspace closes, and Git removes the files in the
background while you keep working.

| | Built-in delete | This plugin |
| --- | --- | --- |
| UI while Git removes the files | Blocked by a modal | Free |
| Workspace closes | After Git finishes | Before Git starts |
| Dirty checkout | Asks to "delete anyway" | Asks to "delete anyway" |
| Branch | Kept | Kept |

## Requirements

- Herdr 0.9.0 or newer
- macOS or Linux
- Git
- A Rust toolchain (`cargo`). The plugin is built on your machine when it is installed. Get one
  from [rustup.rs](https://rustup.rs) or with `brew install rust`.

## Install

1. Install the plugin. Herdr clones the repository, shows what it will run, and builds it.

   ```sh
   herdr plugin install ujizin/herdr-delete-worktree-async
   ```

2. Bind the action to a key in `~/.config/herdr/config.toml`. Any free key works.

   ```toml
   [[keys.command]]
   key = "prefix+backspace"
   type = "plugin_action"
   command = "dev.ujizin.delete-worktree-async.delete"
   description = "Delete worktree checkout..."
   ```

3. Reload the config.

   ```sh
   herdr server reload-config
   ```

To update, run the install command again. To remove the plugin:

```sh
herdr plugin uninstall dev.ujizin.delete-worktree-async
```

## Use

1. Focus the worktree workspace you want to delete.
2. Press the key you bound. A popup shows the checkout folder.
3. Press `Enter` to delete, or `Esc` to cancel.

The popup closes at once. The workspace closes a moment later, and Git deletes the checkout in
the background. The branch is not deleted.

The action only works on worktree workspaces. On any other workspace it shows a notification and
does nothing.

Herdr does not let plugins add items to the sidebar right-click menu, so the action always
targets the focused workspace.

### Modified or untracked files

Before closing the workspace, the plugin checks the checkout with `git status`. If there are
modified or untracked files, the workspace stays open and the popup comes back with
"delete anyway". Press `Enter` to delete the checkout along with those files, or `Esc` to keep
it.

The check takes as long as `git status` takes in that checkout, so on a large repo the workspace
may stay for a few seconds after you confirm.

### If the removal fails

If Git fails after the workspace has closed, a Herdr notification shows Git's message. The
checkout is still on disk. Reopen it with Herdr's "Open worktree...".

### Without a keybinding

```sh
herdr plugin action invoke dev.ujizin.delete-worktree-async.delete
```

### Showing the removing state in the sidebar

Between confirming and the workspace closing, the workspace carries the metadata token
`worktree_remove` with the value `removing…`. Herdr only draws tokens you ask for. To show it, add
`"$worktree_remove"` to a sidebar row:

```toml
[ui.sidebar.spaces]
rows = [["state_icon", "workspace"], ["branch", "git_status", "$worktree_remove"]]
```

## Troubleshooting

| Problem | What to check |
| --- | --- |
| Install fails with `cargo` not found | Install a Rust toolchain, then run the install command again. |
| The key does nothing | Run `herdr server reload-config`, and check the key is not bound to something else. |
| A notification says the workspace is not a linked worktree checkout | The focused workspace is not a worktree created or opened through Herdr. |
| The popup has the wrong colors | See [Theme](#theme). |
| A delete did not happen | Read `worker.log`, described below. |

The background process writes one line per step to
`~/.local/state/herdr/plugins/dev.ujizin.delete-worktree-async/worker.log`. The action itself is
listed by:

```sh
herdr plugin log list --plugin dev.ujizin.delete-worktree-async
```

## How it works

The plugin is one Rust binary with three entry points, declared in `herdr-plugin.toml`.

| Entry point | Started by | Does |
| --- | --- | --- |
| `action` | your keybinding | Reads the focused workspace from `HERDR_PLUGIN_CONTEXT_JSON`, checks it is a linked worktree, and opens the popup. |
| `confirm` | the popup | Draws the confirmation. On `Enter` it starts the worker and exits. |
| `worker` | `confirm` | Runs in its own session with no terminal, so it outlives the popup. |

The worker runs these in order:

1. `git status --porcelain` in the checkout. If it is dirty, stop and ask to force.
2. `herdr workspace close <id>`.
3. `git -C <repo> worktree remove [--force] <path>`.

Herdr's own `worktree.remove` keeps the workspace open until Git returns, which is why the worker
closes the workspace itself and calls Git directly.

### Theme

Herdr does not expose its theme to plugins. The popup reads the theme name from `[theme]` in
`config.toml` and looks its colors up in `src/themes.rs`, a table of Herdr's built-in themes.
Colors in `[theme.custom]` are applied on top.

`[theme.custom.light]`, `[theme.custom.dark]` and `auto_switch` are not applied. A theme the
table does not know leaves the terminal's own colors.

## Limits

- Plugins that listen for `worktree.removed` are not told about these removals, since Herdr does
  not do them. `workspace.closed` still fires.
- The theme table was copied from Herdr 0.9.1. Themes added to Herdr later are unknown to the
  popup until the table is regenerated.
- The "delete anyway" popup cannot open while another Herdr modal is up. The refusal is then
  shown as a notification, and you have to run the action again.
- Windows is not supported.

## Development

```sh
git clone https://github.com/ujizin/herdr-delete-worktree-async
cd herdr-delete-worktree-async
cargo build --release
herdr plugin link "$PWD"
```

`herdr plugin link` does not build, so run `cargo build --release` after each change. The linked
plugin picks up the new binary on the next invocation.

```sh
cargo test
cargo clippy --release
```

## Credits

The colors in `src/themes.rs` and the text of the confirmation come from
[Herdr](https://github.com/herdrdev/herdr), which is licensed under Apache-2.0.

## License

Apache-2.0. See [LICENSE](LICENSE).
