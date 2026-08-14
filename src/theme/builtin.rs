use super::{Theme, ThemeColors};

// Design system notes:
// - search_match/search_current are used as FG colors (underline+bold style, not bg)
//   so they must be high-contrast against the theme bg
// - blockquote_bar is a structural element — needs to be clearly visible
// - code_block_bg should have noticeable contrast from main bg
// - status_bar_bg should be distinctly different from main bg
// - bold should be brighter/whiter than regular fg on dark themes

pub fn dark() -> Theme {
    Theme {
        name: "dark".to_string(),
        code_theme: "base16-ocean.dark".to_string(),
        colors: ThemeColors {
            bg: Some("#1a1b26".to_string()),
            fg: "#c0caf5".to_string(),
            heading1: "#7aa2f7".to_string(),
            heading2: "#7dcfff".to_string(),
            heading3: "#bb9af7".to_string(),
            heading4: "#9ece6a".to_string(),
            heading5: "#e0af68".to_string(),
            heading6: "#f7768e".to_string(),
            bold: "#e6e8f0".to_string(),
            italic: "#c0caf5".to_string(),
            strikethrough: "#565f89".to_string(),
            code_fg: "#a9b1d6".to_string(),
            code_bg: "#24283b".to_string(),
            code_block_bg: "#24283b".to_string(),
            link: "#7aa2f7".to_string(),
            link_url: "#565f89".to_string(),
            blockquote_bar: "#565f89".to_string(),
            blockquote_text: "#a9b1d6".to_string(),
            list_bullet: "#7aa2f7".to_string(),
            list_number: "#7aa2f7".to_string(),
            table_border: "#3b4261".to_string(),
            table_header: "#7dcfff".to_string(),
            hr: "#3b4261".to_string(),
            task_done: "#9ece6a".to_string(),
            task_pending: "#565f89".to_string(),
            search_match: "#e0af68".to_string(),
            search_current: "#ff9e64".to_string(),
            selection_bg: Some("#33467c".to_string()),
            selection_fg: Some("#c0caf5".to_string()),
            status_bar_bg: "#16161e".to_string(),
            status_bar_fg: "#a9b1d6".to_string(),
            toc_active: "#7aa2f7".to_string(),
            toc_inactive: "#565f89".to_string(),
            admonition_note: "#7aa2f7".to_string(),
            admonition_warning: "#e0af68".to_string(),
            admonition_tip: "#9ece6a".to_string(),
            admonition_important: "#bb9af7".to_string(),
            admonition_caution: "#f7768e".to_string(),
        },
    }
}

pub fn light() -> Theme {
    Theme {
        name: "light".to_string(),
        code_theme: "InspiredGitHub".to_string(),
        colors: ThemeColors {
            bg: Some("#ffffff".to_string()),
            fg: "#24292f".to_string(),
            heading1: "#0550ae".to_string(),
            heading2: "#0969da".to_string(),
            heading3: "#8250df".to_string(),
            heading4: "#116329".to_string(),
            heading5: "#953800".to_string(),
            heading6: "#cf222e".to_string(),
            bold: "#1a1a1a".to_string(),
            italic: "#24292f".to_string(),
            strikethrough: "#6e7781".to_string(),
            code_fg: "#24292f".to_string(),
            code_bg: "#eef1f5".to_string(),
            code_block_bg: "#f6f8fa".to_string(),
            link: "#0969da".to_string(),
            link_url: "#57606a".to_string(),
            blockquote_bar: "#0969da".to_string(),
            blockquote_text: "#57606a".to_string(),
            list_bullet: "#0550ae".to_string(),
            list_number: "#0550ae".to_string(),
            table_border: "#d0d7de".to_string(),
            table_header: "#0550ae".to_string(),
            hr: "#d0d7de".to_string(),
            task_done: "#116329".to_string(),
            task_pending: "#6e7781".to_string(),
            // Search: must be visible as FG on white — use warm dark tones
            search_match: "#b35000".to_string(), // burnt orange — visible on white
            search_current: "#cf222e".to_string(), // red — unmistakable current match
            selection_bg: Some("#cfe4ff".to_string()),
            selection_fg: Some("#24292f".to_string()),
            status_bar_bg: "#d0d7de".to_string(),
            status_bar_fg: "#24292f".to_string(),
            toc_active: "#0550ae".to_string(),
            toc_inactive: "#6e7781".to_string(),
            admonition_note: "#0969da".to_string(),
            admonition_warning: "#9a6700".to_string(),
            admonition_tip: "#116329".to_string(),
            admonition_important: "#8250df".to_string(),
            admonition_caution: "#cf222e".to_string(),
        },
    }
}

pub fn dracula() -> Theme {
    Theme {
        name: "dracula".to_string(),
        code_theme: "base16-ocean.dark".to_string(),
        colors: ThemeColors {
            bg: Some("#282a36".to_string()),
            fg: "#f8f8f2".to_string(),
            heading1: "#bd93f9".to_string(),
            heading2: "#8be9fd".to_string(),
            heading3: "#ff79c6".to_string(),
            heading4: "#50fa7b".to_string(),
            heading5: "#f1fa8c".to_string(),
            heading6: "#ffb86c".to_string(),
            bold: "#ffffff".to_string(),
            italic: "#f8f8f2".to_string(),
            strikethrough: "#6272a4".to_string(),
            code_fg: "#f8f8f2".to_string(),
            code_bg: "#21222c".to_string(),
            code_block_bg: "#21222c".to_string(),
            link: "#8be9fd".to_string(),
            link_url: "#6272a4".to_string(),
            blockquote_bar: "#6272a4".to_string(),
            blockquote_text: "#f8f8f2".to_string(),
            list_bullet: "#bd93f9".to_string(),
            list_number: "#bd93f9".to_string(),
            table_border: "#44475a".to_string(),
            table_header: "#8be9fd".to_string(),
            hr: "#44475a".to_string(),
            task_done: "#50fa7b".to_string(),
            task_pending: "#6272a4".to_string(),
            search_match: "#f1fa8c".to_string(), // yellow — pops on dark dracula bg
            search_current: "#ffb86c".to_string(), // orange
            selection_bg: Some("#44475a".to_string()),
            selection_fg: Some("#f8f8f2".to_string()),
            status_bar_bg: "#21222c".to_string(),
            status_bar_fg: "#f8f8f2".to_string(),
            toc_active: "#bd93f9".to_string(),
            toc_inactive: "#6272a4".to_string(),
            admonition_note: "#8be9fd".to_string(),
            admonition_warning: "#f1fa8c".to_string(),
            admonition_tip: "#50fa7b".to_string(),
            admonition_important: "#bd93f9".to_string(),
            admonition_caution: "#ff5555".to_string(),
        },
    }
}

pub fn catppuccin() -> Theme {
    Theme {
        name: "catppuccin".to_string(),
        code_theme: "base16-ocean.dark".to_string(),
        colors: ThemeColors {
            bg: Some("#1e1e2e".to_string()),
            fg: "#cdd6f4".to_string(),
            heading1: "#89b4fa".to_string(),
            heading2: "#74c7ec".to_string(),
            heading3: "#cba6f7".to_string(),
            heading4: "#a6e3a1".to_string(),
            heading5: "#f9e2af".to_string(),
            heading6: "#f38ba8".to_string(),
            bold: "#e4e8f4".to_string(),
            italic: "#cdd6f4".to_string(),
            strikethrough: "#585b70".to_string(),
            code_fg: "#cdd6f4".to_string(),
            code_bg: "#181825".to_string(),
            code_block_bg: "#181825".to_string(),
            link: "#89b4fa".to_string(),
            link_url: "#585b70".to_string(),
            blockquote_bar: "#585b70".to_string(),
            blockquote_text: "#bac2de".to_string(),
            list_bullet: "#89b4fa".to_string(),
            list_number: "#89b4fa".to_string(),
            table_border: "#45475a".to_string(),
            table_header: "#74c7ec".to_string(),
            hr: "#45475a".to_string(),
            task_done: "#a6e3a1".to_string(),
            task_pending: "#585b70".to_string(),
            search_match: "#f9e2af".to_string(), // peach — warm accent
            search_current: "#fab387".to_string(), // orange
            selection_bg: Some("#45475a".to_string()),
            selection_fg: Some("#cdd6f4".to_string()),
            status_bar_bg: "#11111b".to_string(),
            status_bar_fg: "#bac2de".to_string(),
            toc_active: "#89b4fa".to_string(),
            toc_inactive: "#585b70".to_string(),
            admonition_note: "#89b4fa".to_string(),
            admonition_warning: "#f9e2af".to_string(),
            admonition_tip: "#a6e3a1".to_string(),
            admonition_important: "#cba6f7".to_string(),
            admonition_caution: "#f38ba8".to_string(),
        },
    }
}

pub fn nord() -> Theme {
    Theme {
        name: "nord".to_string(),
        code_theme: "base16-ocean.dark".to_string(),
        colors: ThemeColors {
            bg: Some("#2e3440".to_string()),
            fg: "#d8dee9".to_string(),
            heading1: "#88c0d0".to_string(),
            heading2: "#81a1c1".to_string(),
            heading3: "#b48ead".to_string(),
            heading4: "#a3be8c".to_string(),
            heading5: "#ebcb8b".to_string(),
            heading6: "#bf616a".to_string(),
            bold: "#eceff4".to_string(),
            italic: "#d8dee9".to_string(),
            strikethrough: "#4c566a".to_string(),
            code_fg: "#d8dee9".to_string(),
            code_bg: "#3b4252".to_string(),
            code_block_bg: "#3b4252".to_string(),
            link: "#88c0d0".to_string(),
            link_url: "#4c566a".to_string(),
            blockquote_bar: "#616e88".to_string(),
            blockquote_text: "#d8dee9".to_string(),
            list_bullet: "#88c0d0".to_string(),
            list_number: "#88c0d0".to_string(),
            table_border: "#4c566a".to_string(),
            table_header: "#88c0d0".to_string(),
            hr: "#4c566a".to_string(),
            task_done: "#a3be8c".to_string(),
            task_pending: "#4c566a".to_string(),
            search_match: "#ebcb8b".to_string(),   // warm yellow
            search_current: "#d08770".to_string(), // aurora orange
            selection_bg: Some("#434c5e".to_string()),
            selection_fg: Some("#eceff4".to_string()),
            status_bar_bg: "#3b4252".to_string(),
            status_bar_fg: "#d8dee9".to_string(),
            toc_active: "#88c0d0".to_string(),
            toc_inactive: "#4c566a".to_string(),
            admonition_note: "#88c0d0".to_string(),
            admonition_warning: "#ebcb8b".to_string(),
            admonition_tip: "#a3be8c".to_string(),
            admonition_important: "#b48ead".to_string(),
            admonition_caution: "#bf616a".to_string(),
        },
    }
}

pub fn tokyo_night() -> Theme {
    dark()
}

pub fn gruvbox() -> Theme {
    Theme {
        name: "gruvbox".to_string(),
        code_theme: "base16-ocean.dark".to_string(),
        colors: ThemeColors {
            bg: Some("#282828".to_string()),
            fg: "#ebdbb2".to_string(),
            heading1: "#83a598".to_string(),
            heading2: "#8ec07c".to_string(),
            heading3: "#d3869b".to_string(),
            heading4: "#b8bb26".to_string(),
            heading5: "#fabd2f".to_string(),
            heading6: "#fb4934".to_string(),
            bold: "#fbf1c7".to_string(),
            italic: "#ebdbb2".to_string(),
            strikethrough: "#665c54".to_string(),
            code_fg: "#ebdbb2".to_string(),
            code_bg: "#1d2021".to_string(),
            code_block_bg: "#1d2021".to_string(),
            link: "#83a598".to_string(),
            link_url: "#665c54".to_string(),
            blockquote_bar: "#7c6f64".to_string(),
            blockquote_text: "#d5c4a1".to_string(),
            list_bullet: "#83a598".to_string(),
            list_number: "#83a598".to_string(),
            table_border: "#504945".to_string(),
            table_header: "#83a598".to_string(),
            hr: "#504945".to_string(),
            task_done: "#b8bb26".to_string(),
            task_pending: "#665c54".to_string(),
            search_match: "#fabd2f".to_string(), // gruvbox yellow
            search_current: "#fe8019".to_string(), // gruvbox orange
            selection_bg: Some("#504945".to_string()),
            selection_fg: Some("#ebdbb2".to_string()),
            status_bar_bg: "#1d2021".to_string(),
            status_bar_fg: "#ebdbb2".to_string(),
            toc_active: "#83a598".to_string(),
            toc_inactive: "#665c54".to_string(),
            admonition_note: "#83a598".to_string(),
            admonition_warning: "#fabd2f".to_string(),
            admonition_tip: "#b8bb26".to_string(),
            admonition_important: "#d3869b".to_string(),
            admonition_caution: "#fb4934".to_string(),
        },
    }
}

pub fn solarized() -> Theme {
    Theme {
        name: "solarized".to_string(),
        code_theme: "Solarized (dark)".to_string(),
        colors: ThemeColors {
            bg: Some("#002b36".to_string()),
            fg: "#839496".to_string(),
            heading1: "#268bd2".to_string(),
            heading2: "#2aa198".to_string(),
            heading3: "#6c71c4".to_string(),
            heading4: "#859900".to_string(),
            heading5: "#b58900".to_string(),
            heading6: "#dc322f".to_string(),
            bold: "#93a1a1".to_string(),
            italic: "#839496".to_string(),
            strikethrough: "#586e75".to_string(),
            code_fg: "#839496".to_string(),
            code_bg: "#073642".to_string(),
            code_block_bg: "#073642".to_string(),
            link: "#268bd2".to_string(),
            link_url: "#586e75".to_string(),
            blockquote_bar: "#657b83".to_string(),
            blockquote_text: "#839496".to_string(),
            list_bullet: "#268bd2".to_string(),
            list_number: "#268bd2".to_string(),
            table_border: "#586e75".to_string(),
            table_header: "#2aa198".to_string(),
            hr: "#586e75".to_string(),
            task_done: "#859900".to_string(),
            task_pending: "#586e75".to_string(),
            search_match: "#b58900".to_string(), // solarized yellow
            search_current: "#cb4b16".to_string(), // solarized orange
            selection_bg: Some("#135b68".to_string()),
            selection_fg: Some("#eee8d5".to_string()),
            status_bar_bg: "#073642".to_string(),
            status_bar_fg: "#839496".to_string(),
            toc_active: "#268bd2".to_string(),
            toc_inactive: "#586e75".to_string(),
            admonition_note: "#268bd2".to_string(),
            admonition_warning: "#b58900".to_string(),
            admonition_tip: "#859900".to_string(),
            admonition_important: "#6c71c4".to_string(),
            admonition_caution: "#dc322f".to_string(),
        },
    }
}
