use crate::CliResult;
use anyhow::{Context, bail};
use asset_core::domain::{DirectoryPath, UserRole, UserStatus};
use asset_core::port::LocatedUser;
use asset_core::service::UserService;
use clap::{ArgGroup, Args};
use comfy_table::{Table, presets::UTF8_FULL};

#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("operation")
        .required(true)
        .multiple(false)
        .args(["list", "create", "password", "enable", "disable", "show"])
))]
pub(crate) struct UserCommand {
    /// List all users.
    #[arg(long)]
    list: bool,

    /// Create a user and prompt for its password.
    #[arg(long, value_name = "USERNAME")]
    create: Option<String>,

    /// Create an administrator with access to the root workspace.
    #[arg(
        long,
        requires = "create",
        conflicts_with_all = ["list", "password", "enable", "disable", "show"]
    )]
    admin: bool,

    /// Reset a user's password using a hidden interactive prompt.
    #[arg(long, value_name = "USERNAME")]
    password: Option<String>,

    /// Enable a user account.
    #[arg(long, value_name = "USERNAME")]
    enable: Option<String>,

    /// Disable a user account.
    #[arg(long, value_name = "USERNAME")]
    disable: Option<String>,

    /// Show a user's non-sensitive details.
    #[arg(long, value_name = "USERNAME")]
    show: Option<String>,
}

pub(crate) async fn run(command: UserCommand, users: UserService) -> CliResult {
    if command.list {
        print_user_list(&users.list().await?);
    } else if let Some(username) = command.create {
        let role = create_role(command.admin);
        let password = prompt_new_password()?;
        let user = users.create(username, &password, role, None).await?;
        println!("created {} `{}`", role_name(role), user.username());
    } else if let Some(username) = command.password {
        let password = prompt_new_password()?;
        let user = users
            .update_password(&username, &password)
            .await?
            .ok_or_else(|| asset_core::CoreError::not_found("user", &username))?;
        println!("updated password for user `{}`", user.username());
    } else if let Some(username) = command.enable {
        update_status(&users, &username, UserStatus::Active).await?;
    } else if let Some(username) = command.disable {
        update_status(&users, &username, UserStatus::Disabled).await?;
    } else if let Some(username) = command.show {
        let user = users
            .find_located_by_username(&username)
            .await?
            .ok_or_else(|| asset_core::CoreError::not_found("user", &username))?;
        print_user(&user);
    } else {
        unreachable!("clap requires exactly one user operation");
    }
    Ok(())
}

fn create_role(admin: bool) -> UserRole {
    if admin {
        UserRole::Administrator
    } else {
        UserRole::Member
    }
}

async fn update_status(users: &UserService, username: &str, status: UserStatus) -> CliResult {
    let user = users
        .find_by_username(username)
        .await?
        .ok_or_else(|| asset_core::CoreError::not_found("user", username))?;
    let user = users
        .update_status(&user.id(), status)
        .await?
        .ok_or_else(|| asset_core::CoreError::not_found("user", username))?;
    println!(
        "{} user `{}`",
        match status {
            UserStatus::Active => "enabled",
            UserStatus::Disabled => "disabled",
        },
        user.user().username()
    );
    Ok(())
}

fn prompt_new_password() -> CliResult<String> {
    let password = rpassword::prompt_password("Password: ")
        .context("failed to read password from the terminal")?;
    let confirmation = rpassword::prompt_password("Confirm password: ")
        .context("failed to read password confirmation from the terminal")?;
    if password != confirmation {
        bail!("passwords do not match");
    }
    Ok(password)
}

fn print_user_list(users: &[LocatedUser]) {
    println!("{}", user_table(users));
}

fn user_table(users: &[LocatedUser]) -> Table {
    let mut table = Table::new();
    table.load_style(UTF8_FULL);
    table.set_header(["USERNAME", "ROLE", "STATUS", "WORKSPACE", "ID"]);
    for located in users {
        let user = located.user();
        let workspace = located.workspace();
        table.add_row([
            user.username().to_owned(),
            role_name(user.role()).to_owned(),
            status_name(user.status()).to_owned(),
            workspace_name(workspace.path()).to_owned(),
            user.id().to_string(),
        ]);
    }
    table
}

fn print_user(located: &LocatedUser) {
    let user = located.user();
    let workspace = located.workspace();
    println!("Username: {}", user.username());
    println!("ID: {}", user.id());
    println!("Role: {}", role_name(user.role()));
    println!("Status: {}", status_name(user.status()));
    println!("Workspace: {}", workspace_name(workspace.path()));
    println!("Created: {}", user.created_at().to_rfc3339());
    println!("Updated: {}", user.updated_at().to_rfc3339());
}

fn role_name(role: UserRole) -> &'static str {
    match role {
        UserRole::Administrator => "administrator",
        UserRole::Member => "member",
    }
}

fn status_name(status: UserStatus) -> &'static str {
    match status {
        UserStatus::Active => "active",
        UserStatus::Disabled => "disabled",
    }
}

fn workspace_name(workspace: &DirectoryPath) -> &str {
    if workspace.is_root() {
        "/"
    } else {
        workspace.path()
    }
}
