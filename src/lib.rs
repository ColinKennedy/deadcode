pub mod cli;
pub mod constants;
pub mod data_types;

pub mod utils {
    pub mod add_colors_to_diff;
    pub mod fix_indent;
    pub mod fnmatch;
    pub mod line_index;
    pub mod nested_scopes;
    pub mod path_utils;
}

pub mod actions {
    pub mod find_python_filenames;
    pub mod fix_or_show_unused_code;
    pub mod get_unused_names_error_message;
    pub mod merge_overlapping_file_parts;
    pub mod parse_arguments;
    pub mod parse_tach_config;
    pub mod remove_file_parts_from_content;
}

pub mod visitor {
    pub mod code_item;
    pub mod dead_code_visitor;
    pub mod ignore;
    pub mod noqa;
    pub mod utils;
}
