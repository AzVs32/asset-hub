mod actions;
mod cover;

use asset_plugin_sdk::export_directory_action;

export_directory_action!(render_thumbnail => crate::actions::thumbnail::handle);
export_directory_action!(render_workspace => crate::actions::workspace::handle);
export_directory_action!(create_game => crate::actions::create_game::handle);
