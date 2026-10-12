# herdr-delete-worktree-async

A [Herdr](https://herdr.dev) plugin that deletes a worktree checkout in the background.

Herdr's built-in "Delete worktree checkout..." keeps its `removing…` modal up until
`git worktree remove` returns, and the client takes no input meanwhile. On a large repo that is
seconds to minutes ([herdrdev/herdr#1428](https://github.com/herdrdev/herdr/discussions/1428)).

This plugin closes the workspace as soon as you confirm and runs `git worktree remove` from a
detached process, so nothing waits on Git.

## Install

Requires Herdr 0.9.0 or newer, `cargo`, and macOS or Linux.

```sh
herdr plugin install ujizin/herdr-delete-worktree-async
```

From a local checkout:

```sh
cargo build --release
herdr plugin link "$PWD"
```

## Use

Bind the action to a key in `~/.config/herdr/config.toml`, then run `herdr server reload-config`:

```toml
[[keys.command]]
key = "prefix+shift+d"
type = "plugin_action"
command = "dev.ujizin.delete-worktree-async.delete"
description = "Delete worktree checkout..."
```

The action targets the focused workspace. Herdr does not let plugins add items to the sidebar
right-click menu, so focus the worktree workspace first.

1. A popup shows the checkout path. `Enter` confirms, `Esc` cancels.
2. The popup closes. The checkout is checked for modified or untracked files, which takes as
   long as `git status` does there.
3. The workspace closes and Git deletes the checkout in the background. The branch is kept.

If the checkout has modified or untracked files, the workspace stays open and the popup comes
back offering "delete anyway". A failure after the workspace has closed is shown as a Herdr
notification with Git's message; the checkout is still on disk and can be reopened with
"Open worktree...".

The popup is drawn with the colors of the theme in `config.toml`, as Herdr's own overlays are.

It can also be invoked without a keybinding:

```sh
herdr plugin action invoke dev.ujizin.delete-worktree-async.delete
```

### Showing the removing state in the sidebar

Between confirming and the workspace closing, the workspace carries the metadata token
`worktree_remove` with the value `removing…`. Herdr only draws tokens you ask for. Add `"$worktree_remove"` to a sidebar row:

```toml
[ui.sidebar.spaces]
rows = [["state_icon", "workspace"], ["branch", "git_status", "$worktree_remove"]]
```

## How it works

| Entry point | Launched by | Does |
| --- | --- | --- |
| `action` | the `delete` action | Reads `HERDR_PLUGIN_CONTEXT_JSON`, checks the focused workspace is a linked worktree, opens the popup. |
| `confirm` | the `confirm` pane | Draws the confirmation. On `Enter` it starts the worker and exits. |
| `worker` | `confirm` | Runs in its own session with no terminal. Runs `git status --porcelain`, then `herdr workspace close <id>`, then `git -C <repo> worktree remove [--force] <path>`, and reports a failure. |

Herdr's own `worktree.remove` keeps the workspace open until Git returns, so the worker closes
the workspace itself and calls Git directly. `workspace.closed` fires; `worktree.removed` does
not, since Herdr did not do the removal.

Herdr does not expose its theme to plugins. `src/themes.rs` holds the colors of the built-in
themes, copied from Herdr 0.9.1, and `[theme.custom]` colors are applied on top.
`[theme.custom.light]`, `[theme.custom.dark]` and `auto_switch` are not applied.

The worker appends to `worker.log` in the plugin state directory
(`~/.local/state/herdr/plugins/dev.ujizin.delete-worktree-async/`). The `action` command shows up
in `herdr plugin log list --plugin dev.ujizin.delete-worktree-async`.

## Limits

- Plugins that listen for `worktree.removed` are not told about these removals.
- A theme added to Herdr after 0.9.1 is unknown to the popup, which then uses the terminal's
  colors.
- The popup is not opened while another Herdr modal is up. In that case a dirty checkout is
  reported as a notification and the action has to be run again.

## Development

```sh
cargo test
cargo build --release
```

`herdr plugin link` does not build, so rebuild after changing the source.
