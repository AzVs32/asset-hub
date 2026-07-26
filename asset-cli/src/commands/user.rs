use crate::{CliResult, audit};
use anyhow::{Context, bail};
use asset_core::domain::{DirectoryPath, SecurityAuditEventType, User, UserRole, UserStatus};
use asset_core::port::SecurityAuditRepository;
use asset_core::service::UserService;
use clap::{ArgGroup, Args};
use comfy_table::{Table, presets::UTF8_FULL};
use std::sync::Arc;

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

    /// Create a member with workspace `users/<username>` and prompt for its password.
    #[arg(long, value_name = "USERNAME")]
    create: Option<String>,

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

pub(crate) async fn run(
    command: UserCommand,
    users: UserService,
    audit: Arc<dyn SecurityAuditRepository>,
) -> CliResult {
    if command.list {
        print_user_list(&users.list().await?);
    } else if let Some(username) = command.create {
        let password = prompt_new_password()?;
        let user = audit::audited(
            audit.as_ref(),
            SecurityAuditEventType::AuthUserCreate,
            Some(&username),
            users.create(username.clone(), &password, UserRole::Member, None),
        )
        .await?;
        println!("created user `{}`", user.username());
    } else if let Some(username) = command.password {
        let password = prompt_new_password()?;
        let user = audit::audited(
            audit.as_ref(),
            SecurityAuditEventType::AuthUserPassword,
            Some(&username),
            async {
                users
                    .update_password(&username, &password)
                    .await?
                    .ok_or_else(|| asset_core::CoreError::not_found("user", &username))
            },
        )
        .await?;
        println!("updated password for user `{}`", user.username());
    } else if let Some(username) = command.enable {
        update_status(&users, audit.as_ref(), &username, UserStatus::Active).await?;
    } else if let Some(username) = command.disable {
        update_status(&users, audit.as_ref(), &username, UserStatus::Disabled).await?;
    } else if let Some(username) = command.show {
        let user = users
            .find_by_username(&username)
            .await?
            .ok_or_else(|| asset_core::CoreError::not_found("user", &username))?;
        print_user(&user);
    } else {
        unreachable!("clap requires exactly one user operation");
    }
    Ok(())
}

async fn update_status(
    users: &UserService,
    audit: &dyn SecurityAuditRepository,
    username: &str,
    status: UserStatus,
) -> CliResult {
    let user = audit::audited(
        audit,
        SecurityAuditEventType::AuthUserStatus,
        Some(username),
        async {
            let user = users
                .find_by_username(username)
                .await?
                .ok_or_else(|| asset_core::CoreError::not_found("user", username))?;
            users
                .update_status(&user.id(), status)
                .await?
                .ok_or_else(|| asset_core::CoreError::not_found("user", username))
        },
    )
    .await?;
    println!(
        "{} user `{}`",
        match status {
            UserStatus::Active => "enabled",
            UserStatus::Disabled => "disabled",
        },
        user.username()
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

fn print_user_list(users: &[User]) {
    println!("{}", user_table(users));
}

fn user_table(users: &[User]) -> Table {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(["USERNAME", "ROLE", "STATUS", "WORKSPACE", "ID"]);
    for user in users {
        table.add_row([
            user.username().to_owned(),
            role_name(user.role()).to_owned(),
            status_name(user.status()).to_owned(),
            workspace_name(user.workspace_directory().path()).to_owned(),
            user.id().to_string(),
        ]);
    }
    table
}

fn print_user(user: &User) {
    println!("Username: {}", user.username());
    println!("ID: {}", user.id());
    println!("Role: {}", role_name(user.role()));
    println!("Status: {}", status_name(user.status()));
    println!(
        "Workspace: {}",
        workspace_name(user.workspace_directory().path())
    );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_list_is_rendered_as_an_aligned_table() {
        let users = [
            User::new(
                "admin",
                "hash",
                UserRole::Administrator,
                asset_core::domain::DirectoryRef::root(),
            )
            .unwrap(),
            User::new(
                "azvs",
                "hash",
                UserRole::Member,
                asset_core::domain::DirectoryRef::new(
                    asset_core::domain::DirectoryId::new(),
                    DirectoryPath::from_path("users/azvs").unwrap(),
                ),
            )
            .unwrap(),
        ];

        let output = user_table(&users).to_string();
        let widths: Vec<_> = output.lines().map(|line| line.chars().count()).collect();

        assert!(widths.iter().all(|width| *width == widths[0]));
        assert!(output.contains("│ admin    ┆ administrator ┆ active ┆ /          ┆"));
        assert!(output.contains("│ azvs     ┆ member        ┆ active ┆ users/azvs ┆"));
    }
}
