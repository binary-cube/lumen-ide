//! `cargo xtask icon-packs [--check]`: the icon packs' data, from their published npm
//! packages (pinned versions, their licenses checked): for each pack, **only the drawings of
//! our icon names** (the table below: ours → theirs), one per line (`name<TAB>svg`), into
//! `crates/modules/appearance/icon-packs/data/<pack>.svgs`, with the pack's license beside it. What a
//! pack lacks is drawn from our own set. `--check` only tells what each pack has.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::web_data::{curl, unpack};

/// A pack: its id (and data file), its npm package, the version used, its license, and
/// where its drawing of a name is in the package.
struct Pack {
    id: &'static str,
    package: &'static str,
    version: &'static str,
    license: &'static str,
    /// The drawing's path in the package (: the name);  stands for any folder.
    path: &'static str,
}

/// The packs, in the table's column order.
const PACKS: [Pack; 11] = [
    Pack {
        id: "lucide",
        package: "lucide-static",
        version: "1.61.0",
        license: "ISC",
        path: "icons/{}.svg",
    },
    Pack {
        id: "tabler",
        package: "@tabler/icons",
        version: "3.49.0",
        license: "MIT",
        path: "icons/outline/{}.svg",
    },
    Pack {
        id: "phosphor",
        package: "@phosphor-icons/core",
        version: "2.1.1",
        license: "MIT",
        path: "assets/regular/{}.svg",
    },
    Pack {
        id: "material",
        package: "@material-symbols/svg-400",
        version: "0.48.1",
        license: "Apache-2.0",
        path: "outlined/{}.svg",
    },
    Pack {
        id: "remix",
        package: "remixicon",
        version: "4.9.1",
        license: "Apache-2.0",
        path: "icons/*/{}.svg",
    },
    Pack {
        id: "heroicons",
        package: "heroicons",
        version: "2.2.0",
        license: "MIT",
        path: "24/outline/{}.svg",
    },
    Pack {
        id: "fluent",
        package: "@fluentui/svg-icons",
        version: "1.1.344",
        license: "MIT",
        path: "icons/{}_24_regular.svg",
    },
    Pack {
        id: "iconoir",
        package: "iconoir",
        version: "7.12.1",
        license: "MIT",
        path: "icons/regular/{}.svg",
    },
    Pack {
        id: "bootstrap",
        package: "bootstrap-icons",
        version: "1.13.2",
        license: "MIT",
        path: "icons/{}.svg",
    },
    // Their 24-pixel drawings (`-24` in the names below).
    Pack {
        id: "octicons",
        package: "@primer/octicons",
        version: "19.42.0",
        license: "MIT",
        path: "build/svg/{}.svg",
    },
    // Their outline drawings (`-outline` in the names below).
    Pack {
        id: "ionicons",
        package: "ionicons",
        version: "8.1.0",
        license: "MIT",
        path: "dist/svg/{}.svg",
    },
];

/// Our icon names and each pack's name for the same thing (: it has none — ours then).
const NAMES: &[(&str, [&str; 11])] = &[
    (
        "alert",
        [
            "circle-alert",
            "alert-circle",
            "warning-circle",
            "error",
            "error-warning-line",
            "exclamation-circle",
            "error_circle",
            "warning-circle",
            "exclamation-circle",
            "alert-24",
            "alert-circle-outline",
        ],
    ),
    (
        "folder",
        [
            "folder",
            "folder",
            "folder",
            "folder",
            "folder-line",
            "folder",
            "folder",
            "folder",
            "folder",
            "file-directory-24",
            "folder-outline",
        ],
    ),
    (
        "pin",
        [
            "pin",
            "pin",
            "push-pin",
            "keep",
            "pushpin-line",
            "",
            "pin",
            "pin",
            "pin",
            "pin-24",
            "pin-outline",
        ],
    ),
    (
        "globe",
        [
            "globe",
            "world",
            "globe",
            "globe",
            "global-line",
            "globe-alt",
            "globe",
            "globe",
            "globe",
            "globe-24",
            "globe-outline",
        ],
    ),
    (
        "palette",
        [
            "palette",
            "palette",
            "palette",
            "palette",
            "palette-line",
            "swatch",
            "color",
            "palette",
            "palette",
            "paintbrush-24",
            "color-palette-outline",
        ],
    ),
    (
        "file-pen",
        [
            "file-pen",
            "file-pencil",
            "note-pencil",
            "edit_document",
            "file-edit-line",
            "pencil-square",
            "document_edit",
            "page-edit",
            "pencil-square",
            "pencil-24",
            "pencil-outline",
        ],
    ),
    (
        "view-editor",
        [
            "code",
            "code",
            "code",
            "code",
            "code-line",
            "code-bracket",
            "code",
            "code",
            "code",
            "code-24",
            "code-outline",
        ],
    ),
    (
        "view-split",
        [
            "columns-2",
            "layout-columns",
            "columns",
            "splitscreen_right",
            "layout-column-line",
            "view-columns",
            "split_vertical",
            "view-columns-2",
            "columns",
            "columns-24",
            "",
        ],
    ),
    (
        "view-preview",
        [
            "eye",
            "eye",
            "eye",
            "visibility",
            "eye-line",
            "eye",
            "eye",
            "eye",
            "eye",
            "eye-24",
            "eye-outline",
        ],
    ),
    (
        "folder-open",
        [
            "folder-open",
            "folder-open",
            "folder-open",
            "folder_open",
            "folder-open-line",
            "folder-open",
            "folder_open",
            "",
            "folder2-open",
            "file-directory-open-fill-24",
            "folder-open-outline",
        ],
    ),
    (
        "file",
        [
            "file",
            "file",
            "file",
            "draft",
            "file-line",
            "document",
            "document",
            "page",
            "file",
            "file-24",
            "document-outline",
        ],
    ),
    (
        "file-code",
        [
            "file-code",
            "file-code",
            "file-code",
            "code_blocks",
            "file-code-line",
            "code-bracket-square",
            "code_block",
            "code-brackets-square",
            "file-code",
            "file-code-24",
            "code-slash-outline",
        ],
    ),
    (
        "file-json",
        [
            "file-json",
            "json",
            "brackets-curly",
            "data_object",
            "braces-line",
            "",
            "braces",
            "code-brackets",
            "braces",
            "",
            "",
        ],
    ),
    (
        "file-text",
        [
            "file-text",
            "file-text",
            "file-text",
            "description",
            "file-text-line",
            "document-text",
            "document_text",
            "page",
            "file-text",
            "file-24",
            "document-text-outline",
        ],
    ),
    (
        "file-style",
        [
            "file-type",
            "file-type-css",
            "file-css",
            "css",
            "css3-line",
            "",
            "paint_brush",
            "css3",
            "css",
            "paintbrush-24",
            "brush-outline",
        ],
    ),
    (
        "chevron-right",
        [
            "chevron-right",
            "chevron-right",
            "caret-right",
            "chevron_right",
            "arrow-right-s-line",
            "chevron-right",
            "chevron_right",
            "nav-arrow-right",
            "chevron-right",
            "chevron-right-24",
            "chevron-forward-outline",
        ],
    ),
    (
        "chevron-down",
        [
            "chevron-down",
            "chevron-down",
            "caret-down",
            "keyboard_arrow_down",
            "arrow-down-s-line",
            "chevron-down",
            "chevron_down",
            "nav-arrow-down",
            "chevron-down",
            "chevron-down-24",
            "chevron-down-outline",
        ],
    ),
    (
        "chevron-up",
        [
            "chevron-up",
            "chevron-up",
            "caret-up",
            "keyboard_arrow_up",
            "arrow-up-s-line",
            "chevron-up",
            "chevron_up",
            "nav-arrow-up",
            "chevron-up",
            "chevron-up-24",
            "chevron-up-outline",
        ],
    ),
    (
        "chevron-left",
        [
            "chevron-left",
            "chevron-left",
            "caret-left",
            "chevron_left",
            "arrow-left-s-line",
            "chevron-left",
            "chevron_left",
            "nav-arrow-left",
            "chevron-left",
            "chevron-left-24",
            "chevron-back-outline",
        ],
    ),
    (
        "x",
        [
            "x",
            "x",
            "x",
            "close",
            "close-line",
            "x-mark",
            "dismiss",
            "x",
            "x",
            "x-24",
            "close-outline",
        ],
    ),
    (
        "search",
        [
            "search",
            "search",
            "magnifying-glass",
            "search",
            "search-line",
            "magnifying-glass",
            "search",
            "search",
            "search",
            "search-24",
            "search-outline",
        ],
    ),
    (
        "plus",
        [
            "plus",
            "plus",
            "plus",
            "add",
            "add-line",
            "plus",
            "add",
            "plus",
            "plus",
            "plus-24",
            "add-outline",
        ],
    ),
    (
        "cpu",
        [
            "cpu",
            "cpu",
            "cpu",
            "memory",
            "cpu-line",
            "cpu-chip",
            "developer_board",
            "cpu",
            "cpu",
            "cpu-24",
            "hardware-chip-outline",
        ],
    ),
    (
        "memory",
        [
            "memory-stick",
            "device-sd-card",
            "memory",
            "memory_alt",
            "sd-card-mini-line",
            "",
            "",
            "electronics-chip",
            "memory",
            "",
            "",
        ],
    ),
    (
        "gpu",
        [
            "gpu",
            "device-desktop-analytics",
            "graphics-card",
            "developer_board_off",
            "computer-line",
            "",
            "",
            "",
            "gpu-card",
            "device-desktop-24",
            "desktop-outline",
        ],
    ),
    (
        "git-branch",
        [
            "git-branch",
            "git-branch",
            "git-branch",
            "fork_right",
            "git-branch-line",
            "",
            "branch",
            "git-branch",
            "",
            "git-branch-24",
            "git-branch-outline",
        ],
    ),
    (
        "git-commit",
        [
            "git-commit-horizontal",
            "git-commit",
            "git-commit",
            "commit",
            "git-commit-line",
            "",
            "branch_fork",
            "git-commit",
            "",
            "git-commit-24",
            "git-commit-outline",
        ],
    ),
    (
        "indent",
        [
            "indent-increase",
            "indent-increase",
            "text-indent",
            "format_indent_increase",
            "indent-increase",
            "",
            "text_indent_increase",
            "",
            "indent",
            "",
            "",
        ],
    ),
    (
        "line-ending",
        [
            "corner-down-left",
            "corner-down-left",
            "arrow-elbow-down-left",
            "keyboard_return",
            "corner-down-left-line",
            "",
            "arrow_enter_left",
            "",
            "arrow-return-left",
            "",
            "return-down-back-outline",
        ],
    ),
    (
        "encoding",
        [
            "binary",
            "binary",
            "binary",
            "code",
            "text-wrap",
            "",
            "code",
            "",
            "file-binary",
            "file-binary-24",
            "code-outline",
        ],
    ),
    (
        "braces",
        [
            "braces",
            "braces",
            "brackets-curly",
            "data_object",
            "braces-line",
            "",
            "braces",
            "code-brackets",
            "braces",
            "code-24",
            "",
        ],
    ),
    (
        "arrow-left",
        [
            "arrow-left",
            "arrow-left",
            "arrow-left",
            "arrow_back",
            "arrow-left-line",
            "arrow-left",
            "arrow_left",
            "arrow-left",
            "arrow-left",
            "arrow-left-24",
            "arrow-back-outline",
        ],
    ),
    (
        "arrow-right",
        [
            "arrow-right",
            "arrow-right",
            "arrow-right",
            "arrow_forward",
            "arrow-right-line",
            "arrow-right",
            "arrow_right",
            "arrow-right",
            "arrow-right",
            "arrow-right-24",
            "arrow-forward-outline",
        ],
    ),
    (
        "arrow-up",
        [
            "arrow-up",
            "arrow-up",
            "arrow-up",
            "arrow_upward",
            "arrow-up-line",
            "arrow-up",
            "arrow_up",
            "arrow-up",
            "arrow-up",
            "arrow-up-24",
            "arrow-up-outline",
        ],
    ),
    (
        "arrow-down",
        [
            "arrow-down",
            "arrow-down",
            "arrow-down",
            "arrow_downward",
            "arrow-down-line",
            "arrow-down",
            "arrow_down",
            "arrow-down",
            "arrow-down",
            "arrow-down-24",
            "arrow-down-outline",
        ],
    ),
    (
        "crosshair",
        [
            "crosshair",
            "crosshair",
            "crosshair",
            "my_location",
            "focus-3-line",
            "",
            "target",
            "",
            "crosshair",
            "crosshairs-24",
            "locate-outline",
        ],
    ),
    (
        "chevrons-expand",
        [
            "chevrons-up-down",
            "selector",
            "caret-up-down",
            "unfold_more",
            "expand-up-down-line",
            "chevron-up-down",
            "chevron_up_down",
            "sort",
            "chevron-expand",
            "unfold-24",
            "chevron-expand-outline",
        ],
    ),
    (
        "chevrons-collapse",
        [
            "chevrons-down-up",
            "arrows-minimize",
            "arrows-in-line-vertical",
            "unfold_less",
            "contract-up-down-line",
            "",
            "arrow_minimize_vertical",
            "compress-lines",
            "chevron-contract",
            "fold-24",
            "chevron-collapse-outline",
        ],
    ),
    (
        "more-vertical",
        [
            "ellipsis-vertical",
            "dots-vertical",
            "dots-three-vertical",
            "more_vert",
            "more-2-line",
            "ellipsis-vertical",
            "more_vertical",
            "more-vert",
            "three-dots-vertical",
            "",
            "ellipsis-vertical-outline",
        ],
    ),
    (
        "more-horizontal",
        [
            "ellipsis",
            "dots",
            "dots-three",
            "more_horiz",
            "more-line",
            "ellipsis-horizontal",
            "more_horizontal",
            "more-horiz",
            "three-dots",
            "kebab-horizontal-24",
            "ellipsis-horizontal-outline",
        ],
    ),
    (
        "minus",
        [
            "minus",
            "minus",
            "minus",
            "remove",
            "subtract-line",
            "minus",
            "subtract",
            "minus",
            "subtract",
            "dash-24",
            "remove-outline",
        ],
    ),
    (
        "folder-tool",
        [
            "folder-cog",
            "folder-cog",
            "folder-simple-user",
            "folder_managed",
            "folder-settings-line",
            "",
            "",
            "folder-settings",
            "",
            "",
            "",
        ],
    ),
    (
        "terminal",
        [
            "terminal",
            "terminal-2",
            "terminal",
            "terminal",
            "terminal-box-line",
            "command-line",
            "window_dev_tools",
            "terminal",
            "terminal",
            "terminal-24",
            "terminal-outline",
        ],
    ),
    (
        "view-split-down",
        [
            "rows-2",
            "layout-rows",
            "rows",
            "horizontal_split",
            "layout-row-line",
            "",
            "split_horizontal",
            "horizontal-split",
            "",
            "rows-24",
            "",
        ],
    ),
    (
        "copy",
        [
            "copy",
            "copy",
            "copy",
            "content_copy",
            "file-copy-line",
            "document-duplicate",
            "copy",
            "copy",
            "copy",
            "copy-24",
            "copy-outline",
        ],
    ),
    (
        "scissors",
        [
            "scissors",
            "scissors",
            "scissors",
            "content_cut",
            "scissors-line",
            "scissors",
            "cut",
            "cut",
            "scissors",
            "",
            "cut-outline",
        ],
    ),
    (
        "clipboard",
        [
            "clipboard",
            "clipboard",
            "clipboard",
            "content_paste",
            "clipboard-line",
            "clipboard",
            "clipboard",
            "paste-clipboard",
            "clipboard",
            "paste-24",
            "clipboard-outline",
        ],
    ),
    (
        "hash",
        [
            "hash",
            "hash",
            "hash",
            "tag",
            "hashtag",
            "hashtag",
            "number_symbol",
            "hashtag",
            "hash",
            "hash-24",
            "",
        ],
    ),
    (
        "link",
        [
            "link",
            "link",
            "link",
            "link",
            "link",
            "link",
            "link",
            "link",
            "link",
            "link-24",
            "link-outline",
        ],
    ),
    (
        "app-window",
        [
            "app-window",
            "app-window",
            "app-window",
            "web_asset",
            "window-line",
            "window",
            "window",
            "app-window",
            "window",
            "browser-24",
            "browsers-outline",
        ],
    ),
    (
        "rotate-ccw",
        [
            "rotate-ccw",
            "rotate",
            "arrow-counter-clockwise",
            "rotate_left",
            "anticlockwise-line",
            "arrow-uturn-left",
            "arrow_rotate_counterclockwise",
            "undo",
            "arrow-counterclockwise",
            "undo-24",
            "arrow-undo-outline",
        ],
    ),
    (
        "x-square",
        [
            "square-x",
            "square-x",
            "x-square",
            "disabled_by_default",
            "close-circle-line",
            "x-circle",
            "dismiss_square",
            "x-square",
            "x-square",
            "x-circle-24",
            "close-circle-outline",
        ],
    ),
    (
        "arrow-left-to-line",
        [
            "arrow-left-to-line",
            "arrow-bar-to-left",
            "arrow-line-left",
            "first_page",
            "skip-left-line",
            "",
            "arrow_previous",
            "",
            "arrow-bar-left",
            "move-to-start-24",
            "play-skip-back-outline",
        ],
    ),
    (
        "arrow-right-to-line",
        [
            "arrow-right-to-line",
            "arrow-bar-to-right",
            "arrow-line-right",
            "last_page",
            "skip-right-line",
            "",
            "arrow_next",
            "",
            "arrow-bar-right",
            "move-to-end-24",
            "play-skip-forward-outline",
        ],
    ),
    (
        "file-check",
        [
            "file-check",
            "file-check",
            "check-square",
            "task",
            "file-check-line",
            "document-check",
            "document_checkmark",
            "check-square",
            "file-check",
            "file-check-24",
            "checkbox-outline",
        ],
    ),
    ("layers-x", ["", "", "", "", "", "", "", "", "", "", ""]),
    (
        "modules",
        [
            "blocks",
            "puzzle",
            "puzzle-piece",
            "extension",
            "puzzle-line",
            "puzzle-piece",
            "puzzle_piece",
            "puzzle",
            "puzzle",
            "apps-24",
            "extension-puzzle-outline",
        ],
    ),
    ("view-split-move", ["", "", "", "", "", "", "", "", "", "", ""]),
    ("view-split-move-down", ["", "", "", "", "", "", "", "", "", "", ""]),
    (
        "compare",
        [
            "git-compare",
            "git-compare",
            "git-diff",
            "compare",
            "git-pull-request-line",
            "",
            "branch_compare",
            "git-compare",
            "file-diff",
            "git-compare-24",
            "git-compare-outline",
        ],
    ),
    (
        "graph",
        [
            "chart-line",
            "chart-line",
            "chart-line",
            "show_chart",
            "line-chart-line",
            "chart-bar",
            "data_trending",
            "graph-up",
            "graph-up",
            "graph-24",
            "analytics-outline",
        ],
    ),
    (
        "arrow-up-right",
        [
            "arrow-up-right",
            "arrow-up-right",
            "arrow-up-right",
            "north_east",
            "arrow-right-up-line",
            "arrow-up-right",
            "arrow_up_right",
            "arrow-up-right",
            "arrow-up-right",
            "arrow-up-right-24",
            "open-outline",
        ],
    ),
    (
        "pipette",
        [
            "pipette",
            "color-picker",
            "eyedropper",
            "colorize",
            "drop-line",
            "",
            "eyedropper",
            "color-picker",
            "eyedropper",
            "",
            "eyedrop-outline",
        ],
    ),
    (
        "keyboard",
        [
            "keyboard",
            "keyboard",
            "keyboard",
            "keyboard",
            "keyboard-line",
            "",
            "keyboard",
            "",
            "keyboard",
            "",
            "",
        ],
    ),
    (
        "trash",
        [
            "trash-2",
            "trash",
            "trash",
            "delete",
            "delete-bin-line",
            "trash",
            "delete",
            "trash",
            "trash",
            "trash-24",
            "trash-outline",
        ],
    ),
    (
        "clock",
        [
            "clock",
            "clock",
            "clock",
            "schedule",
            "time-line",
            "clock",
            "clock",
            "clock",
            "clock",
            "clock-24",
            "time-outline",
        ],
    ),
    (
        "download",
        [
            "download",
            "download",
            "download-simple",
            "download",
            "download-line",
            "arrow-down-tray",
            "arrow_download",
            "download",
            "download",
            "download-24",
            "download-outline",
        ],
    ),
    (
        "folder-input",
        [
            "folder-input",
            "folder-down",
            "folder-simple-plus",
            "drive_file_move",
            "folder-transfer-line",
            "folder-arrow-down",
            "folder_arrow_right",
            "",
            "",
            "",
            "",
        ],
    ),
    (
        "tag",
        [
            "tag",
            "tag",
            "tag",
            "sell",
            "price-tag-3-line",
            "tag",
            "tag",
            "label",
            "tag",
            "tag-24",
            "pricetag-outline",
        ],
    ),
    (
        "eye",
        [
            "eye",
            "eye",
            "eye",
            "visibility",
            "eye-line",
            "eye",
            "eye",
            "eye",
            "eye",
            "eye-24",
            "eye-outline",
        ],
    ),
    (
        "eye-off",
        [
            "eye-off",
            "eye-off",
            "eye-slash",
            "visibility_off",
            "eye-off-line",
            "eye-slash",
            "eye_off",
            "eye-closed",
            "eye-slash",
            "eye-closed-24",
            "eye-off-outline",
        ],
    ),
    (
        "server",
        [
            "server",
            "server",
            "hard-drives",
            "dns",
            "server-line",
            "server",
            "server",
            "server",
            "server",
            "server-24",
            "server-outline",
        ],
    ),
    (
        "cloud",
        [
            "cloud",
            "cloud",
            "cloud",
            "cloud",
            "cloud-line",
            "cloud",
            "cloud",
            "cloud",
            "cloud",
            "cloud-24",
            "cloud-outline",
        ],
    ),
    (
        "square",
        [
            "square",
            "square",
            "square",
            "check_box_outline_blank",
            "checkbox-blank-line",
            "stop",
            "square",
            "square",
            "square",
            "square-24",
            "square-outline",
        ],
    ),
    (
        "upload",
        [
            "upload",
            "upload",
            "upload-simple",
            "upload",
            "upload-line",
            "arrow-up-tray",
            "arrow_upload",
            "upload",
            "upload",
            "upload-24",
            "cloud-upload-outline",
        ],
    ),
    (
        "refresh",
        [
            "refresh-cw",
            "refresh",
            "arrows-clockwise",
            "refresh",
            "refresh-line",
            "arrow-path",
            "arrow_clockwise",
            "refresh",
            "arrow-clockwise",
            "sync-24",
            "refresh-outline",
        ],
    ),
    (
        "user",
        [
            "user",
            "user",
            "user",
            "person",
            "user-line",
            "user",
            "person",
            "user",
            "person",
            "person-24",
            "person-outline",
        ],
    ),
    (
        "lock",
        [
            "lock",
            "lock",
            "lock",
            "lock",
            "lock-line",
            "lock-closed",
            "lock_closed",
            "lock",
            "lock",
            "lock-24",
            "lock-closed-outline",
        ],
    ),
    (
        "key",
        [
            "key-round",
            "key",
            "key",
            "key",
            "key-2-line",
            "key",
            "key",
            "key",
            "key",
            "key-24",
            "key-outline",
        ],
    ),
    (
        "maximize-2",
        [
            "maximize-2",
            "arrows-diagonal",
            "arrows-out-simple",
            "open_in_full",
            "expand-diagonal-line",
            "arrows-pointing-out",
            "arrow_maximize",
            "maximize",
            "arrows-angle-expand",
            "maximize-24",
            "expand-outline",
        ],
    ),
    (
        "minimize-2",
        [
            "minimize-2",
            "arrows-diagonal-minimize-2",
            "arrows-in-simple",
            "close_fullscreen",
            "collapse-diagonal-line",
            "arrows-pointing-in",
            "arrow_minimize",
            "reduce",
            "arrows-angle-contract",
            "minimize-24",
            "contract-outline",
        ],
    ),
    (
        "archive",
        [
            "file-archive",
            "file-zip",
            "file-archive",
            "folder_zip",
            "file-zip-line",
            "archive-box",
            "folder_zip",
            "archive",
            "file-zip",
            "file-zip-24",
            "archive-outline",
        ],
    ),
    (
        "database",
        [
            "database",
            "database",
            "database",
            "database",
            "database-2-line",
            "circle-stack",
            "database",
            "database",
            "database",
            "database-24",
            "server-outline",
        ],
    ),
    (
        "docker",
        [
            "container",
            "brand-docker",
            "",
            "deployed_code",
            "",
            "",
            "",
            "cube",
            "box-seam",
            "container-24",
            "cube-outline",
        ],
    ),
    (
        "file-lines",
        [
            "file-text",
            "file-text",
            "file-text",
            "article",
            "file-list-line",
            "document-text",
            "document_text",
            "page",
            "file-text",
            "file-24",
            "document-text-outline",
        ],
    ),
    ("file-twig", ["", "", "", "", "", "", "", "leaf", "", "", ""]),
    (
        "git",
        [
            "git-branch",
            "brand-git",
            "git-branch",
            "",
            "git-repository-line",
            "",
            "",
            "git-branch",
            "git",
            "git-branch-24",
            "git-branch-outline",
        ],
    ),
    (
        "image",
        [
            "image",
            "photo",
            "image",
            "image",
            "image-line",
            "photo",
            "image",
            "media-image",
            "image",
            "image-24",
            "image-outline",
        ],
    ),
    (
        "layers",
        [
            "layers",
            "stack-2",
            "stack",
            "layers",
            "stack-line",
            "square-3-stack-3d",
            "layer",
            "",
            "layers",
            "stack-24",
            "layers-outline",
        ],
    ),
    (
        "markdown",
        [
            "file-text",
            "markdown",
            "markdown-logo",
            "",
            "markdown-line",
            "",
            "",
            "",
            "markdown",
            "markdown-24",
            "document-text-outline",
        ],
    ),
    (
        "package",
        [
            "package",
            "package",
            "package",
            "package_2",
            "box-3-line",
            "cube",
            "box",
            "package",
            "box",
            "package-24",
            "cube-outline",
        ],
    ),
    (
        "settings",
        [
            "settings",
            "settings",
            "gear",
            "settings",
            "settings-3-line",
            "cog-6-tooth",
            "settings",
            "settings",
            "gear",
            "gear-24",
            "settings-outline",
        ],
    ),
    (
        "structure",
        [
            "list-tree",
            "list-tree",
            "tree-structure",
            "account_tree",
            "node-tree",
            "",
            "text_bullet_list_tree",
            "view-structure-down",
            "diagram-3",
            "workflow-24",
            "git-network-outline",
        ],
    ),
    ("img-actual", ["", "", "", "", "", "", "", "", "aspect-ratio", "", ""]),
    ("img-background", ["", "", "", "", "", "", "", "view-grid", "grid-3x3", "", "grid-outline"]),
    (
        "img-fit",
        [
            "maximize",
            "maximize",
            "arrows-out",
            "fit_screen",
            "fullscreen-line",
            "arrows-pointing-out",
            "arrow_maximize",
            "scale-frame-enlarge",
            "arrows-fullscreen",
            "maximize-24",
            "scan-outline",
        ],
    ),
    (
        "img-flip-h",
        [
            "flip-horizontal-2",
            "flip-horizontal",
            "flip-horizontal",
            "flip",
            "",
            "",
            "flip_horizontal",
            "flip",
            "",
            "mirror-24",
            "swap-horizontal-outline",
        ],
    ),
    (
        "img-flip-v",
        [
            "flip-vertical-2",
            "flip-vertical",
            "flip-vertical",
            "",
            "",
            "",
            "flip_vertical",
            "",
            "",
            "",
            "swap-vertical-outline",
        ],
    ),
    (
        "img-rotate-ccw",
        [
            "rotate-ccw",
            "rotate",
            "arrow-counter-clockwise",
            "rotate_left",
            "anticlockwise-line",
            "arrow-uturn-left",
            "arrow_rotate_counterclockwise",
            "rotate-camera-left",
            "arrow-counterclockwise",
            "undo-24",
            "arrow-undo-outline",
        ],
    ),
    (
        "img-rotate-cw",
        [
            "rotate-cw",
            "rotate-clockwise",
            "arrow-clockwise",
            "rotate_right",
            "clockwise-line",
            "arrow-uturn-right",
            "arrow_rotate_clockwise",
            "rotate-camera-right",
            "arrow-clockwise",
            "redo-24",
            "arrow-redo-outline",
        ],
    ),
    (
        "img-save",
        [
            "save",
            "device-floppy",
            "floppy-disk",
            "save",
            "save-line",
            "",
            "save",
            "floppy-disk",
            "save",
            "",
            "save-outline",
        ],
    ),
    (
        "img-zoom-in",
        [
            "zoom-in",
            "zoom-in",
            "magnifying-glass-plus",
            "zoom_in",
            "zoom-in-line",
            "magnifying-glass-plus",
            "zoom_in",
            "zoom-in",
            "zoom-in",
            "zoom-in-24",
            "add-circle-outline",
        ],
    ),
    (
        "img-zoom-out",
        [
            "zoom-out",
            "zoom-out",
            "magnifying-glass-minus",
            "zoom_out",
            "zoom-out-line",
            "magnifying-glass-minus",
            "zoom_out",
            "zoom-out",
            "zoom-out",
            "zoom-out-24",
            "remove-circle-outline",
        ],
    ),
];

pub fn generate(root: &Path, check: bool) -> Result<String, String> {
    let work = root.join("target").join("icon-packs");
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let out =
        root.join("crates").join("modules").join("appearance").join("icon-packs").join("data");
    if !check {
        std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    }
    let mut said = Vec::new();
    for (column, pack) in PACKS.iter().enumerate() {
        let url = format!("https://registry.npmjs.org/{}/{}", pack.package, pack.version);
        let meta: Value = serde_json::from_slice(&curl(&url)?).map_err(|e| e.to_string())?;
        let (dir, _) = unpack(pack.package, &meta, pack.license, &work)?;
        let mut lines = Vec::new();
        let mut missing = Vec::new();
        for (ours, theirs) in NAMES {
            let name = theirs[column];
            if name.is_empty() {
                continue;
            }
            match find(&dir, pack.path, name).and_then(|p| std::fs::read_to_string(p).ok()) {
                Some(svg) => lines.push(format!("{ours}\t{}", compact(&svg))),
                None => missing.push(format!("{ours}={name}")),
            }
        }
        if !missing.is_empty() {
            return Err(format!(
                "{}: not in {} {}: {}",
                pack.id,
                pack.package,
                pack.version,
                missing.join(" ")
            ));
        }
        said.push(format!("{} {}/{}", pack.id, lines.len(), NAMES.len()));
        if check {
            continue;
        }
        let header = format!(
            "# {} {} ({}): only the drawings of our icon names\n",
            pack.package, pack.version, pack.license
        );
        std::fs::write(out.join(format!("{}.svgs", pack.id)), header + &lines.join("\n") + "\n")
            .map_err(|e| e.to_string())?;
        let target = out.join(format!("{}.LICENSE", pack.id));
        let file = ["LICENSE", "LICENSE.md", "LICENSE.txt", "license"]
            .iter()
            .map(|f| dir.join(f))
            .find(|p| p.exists());
        match file {
            Some(file) => std::fs::copy(file, &target).map(drop),
            // Declared in its package.json only (Fluent's): the declaration, with where from.
            None => {
                let repository = meta["repository"]["url"].as_str().unwrap_or("");
                let author = meta["author"]["name"].as_str().or(meta["author"].as_str());
                let note = format!(
                    "{} {}\nLicense: {} (declared in its package.json)\nAuthor: {}\nRepository: {}\n",
                    pack.package,
                    pack.version,
                    pack.license,
                    author.unwrap_or("its authors"),
                    repository
                );
                std::fs::write(&target, note)
            }
        }
        .map_err(|e| e.to_string())?;
    }
    for set in &FILE_SETS {
        said.push(file_set(root, set, check)?);
    }
    Ok(said.join(", "))
}

/// Where a file icon set comes from: its theme's file (VS Code's icon theme format) in an npm
/// package, or in a VS Code extension published on Open VSX.
enum Source {
    Npm {
        package: &'static str,
        version: &'static str,
        theme: &'static str,
    },
    OpenVsx {
        publisher: &'static str,
        name: &'static str,
        version: &'static str,
        theme: &'static str,
    },
}

/// A file icon set: its id (its data files: `<id>-files.map`, `.svgs`, `.LICENSE`), where
/// it comes from and its license. Only its rules for the files and folders common in our
/// projects are kept (and only the drawings those rules use) — not its thousands.
struct FileSet {
    id: &'static str,
    source: Source,
    license: &'static str,
}

const FILE_SETS: [FileSet; 4] = [
    FileSet {
        id: "material",
        source: Source::Npm {
            package: "material-icon-theme",
            version: "5.39.0",
            theme: "dist/material-icons.json",
        },
        license: "MIT",
    },
    // Its Mocha flavor (pastel, readable on dark and light themes).
    FileSet {
        id: "catppuccin",
        source: Source::OpenVsx {
            publisher: "Catppuccin",
            name: "catppuccin-vsc-icons",
            version: "1.26.0",
            theme: "dist/mocha/theme.json",
        },
        license: "MIT",
    },
    FileSet {
        id: "symbols",
        source: Source::OpenVsx {
            publisher: "miguelsolorio",
            name: "symbols",
            version: "0.0.26",
            theme: "src/symbol-icon-theme.modified.json",
        },
        license: "MIT",
    },
    FileSet {
        id: "vscode",
        source: Source::OpenVsx {
            publisher: "vscode-icons-team",
            name: "vscode-icons",
            version: "13.0.0",
            theme: "dist/src/vsicons-icon-theme.json",
        },
        license: "MIT",
    },
];

/// An extension from Open VSX, unpacked under `work` (once): its folder, after checking
/// its license (its manifest's, else its license file's first line).
fn open_vsx(
    publisher: &str,
    name: &str,
    version: &str,
    expected: &str,
    work: &Path,
) -> Result<PathBuf, String> {
    let dir = work.join(format!("{publisher}.{name}-{version}"));
    let extension = dir.join("extension");
    if !extension.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let url = format!(
            "https://open-vsx.org/api/{publisher}/{name}/{version}/file/{publisher}.{name}-{version}.vsix"
        );
        let archive = dir.join("extension.vsix");
        std::fs::write(&archive, curl(&url)?).map_err(|e| e.to_string())?;
        let status = std::process::Command::new("unzip")
            .args(["-q", "-o"])
            .arg(&archive)
            .arg("-d")
            .arg(&dir)
            .status()
            .map_err(|e| format!("unzip: {e}"))?;
        if !status.success() {
            return Err(format!("unzip failed for {publisher}.{name}"));
        }
    }
    let manifest = read_json(&extension.join("package.json"))?;
    let declared = manifest["license"].as_str().map(str::to_owned);
    let license = declared.or_else(|| {
        let text = std::fs::read_to_string(extension.join("LICENSE.txt")).ok()?;
        text.lines()
            .find(|l| !l.trim().is_empty())
            .map(|l| l.trim().trim_end_matches(" License").to_owned())
    });
    if license.as_deref() != Some(expected) {
        return Err(format!("{publisher}.{name}: license is {license:?}, expected {expected}"));
    }
    Ok(extension)
}

/// The extensions kept (theirs: `blade.php`, `d.ts` too).
const EXTENSIONS: &[&str] = &[
    "php",
    "phtml",
    "blade.php",
    "twig",
    "html",
    "htm",
    "js",
    "mjs",
    "cjs",
    "ts",
    "mts",
    "d.ts",
    "tsx",
    "jsx",
    "vue",
    "svelte",
    "css",
    "scss",
    "sass",
    "less",
    "json",
    "jsonc",
    "yaml",
    "yml",
    "cc",
    "cxx",
    "hh",
    "hxx",
    "lua",
    "zig",
    "zon",
    "ksh",
    "command",
    "dockerfile",
    "containerfile",
    "xml",
    "svg",
    "png",
    "jpg",
    "jpeg",
    "gif",
    "webp",
    "ico",
    "bmp",
    "md",
    "markdown",
    "txt",
    "log",
    "sql",
    "db",
    "sqlite",
    "env",
    "ini",
    "conf",
    "toml",
    "lock",
    "sh",
    "bash",
    "zsh",
    "bat",
    "ps1",
    "py",
    "rb",
    "go",
    "rs",
    "java",
    "kt",
    "c",
    "h",
    "cpp",
    "hpp",
    "cs",
    "swift",
    "dart",
    "zip",
    "gz",
    "tar",
    "rar",
    "7z",
    "pdf",
    "csv",
    "xlsx",
    "docx",
    "woff",
    "woff2",
    "ttf",
    "otf",
    "eot",
    "mp4",
    "webm",
    "mp3",
    "wav",
    "graphql",
    "gql",
    "proto",
    "neon",
    "latte",
    "tpl",
    "map",
    "http",
    "pem",
    "key",
    "crt",
    "vim",
    "dist",
];

/// The file names kept (lowercase, as theirs).
const FILE_NAMES: &[&str] = &[
    "composer.json",
    "composer.lock",
    "package.json",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    ".gitignore",
    ".gitattributes",
    ".gitmodules",
    ".editorconfig",
    ".env",
    ".env.example",
    ".env.local",
    "dockerfile",
    ".dockerignore",
    "docker-compose.yml",
    "docker-compose.yaml",
    "compose.yaml",
    "makefile",
    "cmakelists.txt",
    "compile_commands.json",
    ".clangd",
    ".clang-format",
    ".luarc.json",
    "build.zig",
    "build.zig.zon",
    "containerfile",
    "compose.yml",
    ".bashrc",
    ".bash_profile",
    ".zshrc",
    ".profile",
    "pyproject.toml",
    ".taplo.toml",
    "rustfmt.toml",
    "readme.md",
    "license",
    "license.md",
    "changelog.md",
    "tsconfig.json",
    "jsconfig.json",
    "vite.config.js",
    "vite.config.ts",
    "webpack.config.js",
    "webpack.mix.js",
    "tailwind.config.js",
    "tailwind.config.ts",
    "postcss.config.js",
    ".prettierrc",
    ".eslintrc",
    ".eslintrc.json",
    "eslint.config.js",
    "phpunit.xml",
    "phpunit.xml.dist",
    "phpstan.neon",
    "phpstan.neon.dist",
    ".php-cs-fixer.php",
    ".php-cs-fixer.dist.php",
    "artisan",
    ".htaccess",
    "robots.txt",
    "favicon.ico",
    ".babelrc",
    ".npmrc",
    ".nvmrc",
    ".browserslistrc",
    "vagrantfile",
    ".gitlab-ci.yml",
    "symfony.lock",
    "jest.config.js",
    "vitest.config.ts",
    "nginx.conf",
    "cargo.toml",
    "cargo.lock",
];

/// The folder names kept.
const FOLDER_NAMES: &[&str] = &[
    "src",
    "source",
    "test",
    "tests",
    "spec",
    "config",
    "configs",
    "public",
    "assets",
    "images",
    "img",
    "icons",
    "css",
    "styles",
    "sass",
    "scss",
    "js",
    "scripts",
    "vendor",
    "node_modules",
    "app",
    "resources",
    "views",
    "templates",
    "components",
    "lib",
    "libs",
    "docs",
    "doc",
    "dist",
    "build",
    "out",
    ".github",
    ".vscode",
    ".idea",
    "database",
    "migrations",
    "seeders",
    "seeds",
    "routes",
    "storage",
    "cache",
    "logs",
    "log",
    "bin",
    "tools",
    "api",
    "controllers",
    "models",
    "services",
    "utils",
    "helpers",
    "i18n",
    "lang",
    "locales",
    "translations",
    "fonts",
    "docker",
    ".git",
    "temp",
    "tmp",
    "layouts",
    "pages",
    "hooks",
    "middleware",
    "plugins",
    "modules",
    "packages",
    "examples",
    "fixtures",
    "mocks",
    "themes",
    "uploads",
    "var",
    "web",
    "www",
    "client",
    "server",
    "admin",
    "auth",
    "events",
    "jobs",
    "mail",
    "providers",
    "console",
    "http",
    "requests",
    "entity",
    "entities",
    "repository",
    "repositories",
    "migrations",
    "fixtures",
];

/// Our languages (their ids), for files known only by their language.
const LANGUAGES: &[&str] = &[
    "php",
    "javascript",
    "typescript",
    "css",
    "scss",
    "html",
    "json",
    "yaml",
    "twig",
    "blade",
    "markdown",
    "xml",
    "c",
    "cpp",
    "lua",
    "zig",
    "bash",
    "toml",
    "dockerfile",
];

/// Our language ids the sets know by another (VS Code's) id.
const LANGUAGE_ALIASES: &[(&str, &str)] = &[("bash", "shellscript")];

/// A file icon set's data: `<id>-files.map` (`kind<TAB>what<TAB>icon`: `e` an extension, `n`
/// a file name, `l` a language, `f` a folder, `o` an open folder, `d` the defaults) and
/// `<id>-files.svgs` (only the drawings those use), with the license.
fn file_set(root: &Path, set: &FileSet, check: bool) -> Result<String, String> {
    let work = root.join("target").join("icon-packs");
    let out =
        root.join("crates").join("modules").join("appearance").join("icon-packs").join("data");
    let (dir, theme, said_from) = match set.source {
        Source::Npm { package, version, theme } => {
            let url = format!("https://registry.npmjs.org/{package}/{version}");
            let meta: Value = serde_json::from_slice(&curl(&url)?).map_err(|e| e.to_string())?;
            let (dir, _) = unpack(package, &meta, set.license, &work)?;
            (dir, theme, format!("{package} {version}"))
        }
        Source::OpenVsx { publisher, name, version, theme } => {
            let dir = open_vsx(publisher, name, version, set.license, &work)?;
            (dir, theme, format!("{publisher}.{name} {version}"))
        }
    };
    let theme = dir.join(theme);
    let manifest = read_json(&theme)?;
    let base = theme.parent().unwrap_or(&dir).to_path_buf();
    let mut map = Vec::new();
    let mut used = std::collections::BTreeSet::new();
    // A rule naming a drawing the set doesn't have (Symbols' `less`) is left out.
    let drawn = |icon: &&str| manifest["iconDefinitions"][*icon]["iconPath"].is_string();
    let mut keep = |kind: &str, what: &str, icon: Option<&str>| {
        if let Some(icon) = icon.filter(drawn) {
            map.push(format!("{kind}\t{what}\t{icon}"));
            used.insert(icon.to_owned());
        }
    };
    let get = |table: &str, key: &str| manifest[table][key].as_str();
    for ext in EXTENSIONS {
        keep("e", ext, get("fileExtensions", ext));
    }
    for name in FILE_NAMES {
        keep("n", name, get("fileNames", name));
    }
    for language in LANGUAGES {
        let theirs = LANGUAGE_ALIASES.iter().find(|(ours, _)| ours == language).map(|(_, id)| *id);
        keep("l", language, get("languageIds", language).or_else(|| get("languageIds", theirs?)));
    }
    for folder in FOLDER_NAMES {
        keep("f", folder, get("folderNames", folder));
        keep("o", folder, get("folderNamesExpanded", folder).or(get("folderNames", folder)));
    }
    // A set without open folders (Symbols') shows them closed.
    let folder = manifest["folder"].as_str();
    for (what, icon) in [
        ("file", manifest["file"].as_str()),
        ("folder", folder),
        ("folder-open", manifest["folderExpanded"].as_str().or(folder)),
    ] {
        keep("d", what, icon);
    }
    let mut svgs = Vec::new();
    for icon in &used {
        let path = manifest["iconDefinitions"][icon]["iconPath"]
            .as_str()
            .ok_or_else(|| format!("{}: no drawing for {icon}", set.id))?;
        let file = base.join(path);
        let svg = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        svgs.push(format!("{icon}\t{}", compact(&svg)));
    }
    let said = format!("{}-files {} rules, {} drawings", set.id, map.len(), svgs.len());
    if check {
        return Ok(said);
    }
    let header = format!("# {said_from} ({}): only the rules and drawings we keep\n", set.license);
    let write = |kind: &str, lines: &[String]| {
        let file = out.join(format!("{}-files.{kind}", set.id));
        std::fs::write(file, header.clone() + &lines.join("\n") + "\n").map_err(|e| e.to_string())
    };
    write("map", &map)?;
    write("svgs", &svgs)?;
    let license = ["LICENSE", "LICENSE.md", "LICENSE.txt"]
        .iter()
        .map(|f| dir.join(f))
        .find(|p| p.exists())
        .ok_or_else(|| format!("{}: no license file", set.id))?;
    std::fs::copy(license, out.join(format!("{}-files.LICENSE", set.id)))
        .map_err(|e| e.to_string())?;
    Ok(said)
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The drawing of `name` in the unpacked package, by the pack's path (`*`: any folder).
fn find(dir: &Path, path: &str, name: &str) -> Option<PathBuf> {
    let path = path.replace("{}", name);
    let Some((head, tail)) = path.split_once("/*/") else {
        let file = dir.join(&path);
        return file.exists().then_some(file);
    };
    std::fs::read_dir(dir.join(head))
        .ok()?
        .flatten()
        .map(|e| e.path().join(tail))
        .find(|p| p.exists())
}

/// An SVG on one line: no comments, no line breaks, spaces between tags gone.
fn compact(svg: &str) -> String {
    let mut text = svg.to_owned();
    while let (Some(a), Some(b)) = (text.find("<!--"), text.find("-->")) {
        if b < a {
            break;
        }
        text.replace_range(a..b + 3, "");
    }
    let joined: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    joined.join(" ").replace("> <", "><").replace('\t', " ")
}
