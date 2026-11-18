//! Who the files written on the server belong to (step 3): as the server makes them, as the
//! project's folder, or a chosen owner and group — from the server's accounts, read by the
//! browsing session (never on the UI thread), in a list to choose from. FTP can't change
//! owners: said instead of the choices.

use remote::properties::{Accounts, Principal, Properties};
use ui_components::picker::{PickerColumn, PickerEvent, PickerItem, TableRows};

use super::*;

/// One of the two lists: the owner's, the group's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Account {
    Owner,
    Group,
}

/// The three ways, in their pills' order.
const KINDS: [&str; 3] =
    ["remote.write_as_server", "remote.write_as_project", "remote.write_as_chosen"];

/// The server's accounts for the lists, and the project folder's owner and group.
#[derive(Debug, Default, Clone)]
pub(super) struct ServerAccounts {
    pub accounts: Accounts,
    pub project: Option<Properties>,
}

/// What the browsing session reads for the lists: the accounts, and those the project's
/// folder (`dir`, from the server's root) has (added: the lists may lack them).
pub(super) fn read_accounts(
    fs: &mut dyn remote::application::RemoteFs,
    dir: &str,
) -> ServerAccounts {
    let relative = dir.trim_start_matches('/').to_owned();
    let project = fs.properties(&[relative]).ok().and_then(|p| p.into_iter().next().flatten());
    let owner = project.as_ref().and_then(|p| p.owner.clone());
    let group = project.as_ref().and_then(|p| p.group.clone());
    let accounts = fs.accounts().unwrap_or_default().with_seen(owner.iter(), group.iter());
    ServerAccounts { accounts, project }
}

impl ConnectView {
    /// Owners can be changed over this protocol.
    pub(super) fn owners_changeable(&self) -> bool {
        self.protocol == Protocol::Sftp
    }

    /// The pill of `write_as`.
    fn kind_of(write_as: &WriteAs) -> usize {
        match write_as {
            WriteAs::Server => 0,
            WriteAs::Project => 1,
            WriteAs::Chosen { .. } => 2,
        }
    }

    /// The accounts asked of the browsing session (for the chosen folder).
    pub(super) fn ask_accounts(&mut self) {
        if !self.owners_changeable() {
            return;
        }
        self.server_accounts = None;
        if let Some((tx, _)) = &self.browser {
            let _ = tx.send(Request::Accounts(self.root.clone()));
        }
    }

    /// A pill clicked.
    pub(super) fn write_as_kind(&mut self, i: usize) {
        self.write_as = match i {
            0 => WriteAs::Server,
            1 => WriteAs::Project,
            _ => match &self.write_as {
                chosen @ WriteAs::Chosen { .. } => chosen.clone(),
                _ => WriteAs::Chosen { owner: None, group: None },
            },
        };
    }

    fn chosen(&self, account: Account) -> Option<&Principal> {
        match (&self.write_as, account) {
            (WriteAs::Chosen { owner, .. }, Account::Owner) => owner.as_ref(),
            (WriteAs::Chosen { group, .. }, Account::Group) => group.as_ref(),
            _ => None,
        }
    }

    fn accounts_of(&self, account: Account) -> &[Principal] {
        let Some(read) = &self.server_accounts else { return &[] };
        match account {
            Account::Owner => &read.accounts.users,
            Account::Group => &read.accounts.groups,
        }
    }

    /// A list opened from its field: "as the server makes it" first, then each account.
    pub(super) fn open_accounts(&mut self, account: Account) {
        let accounts = self.accounts_of(account);
        let mut items = vec![PickerItem::person(t("remote.write_as_unset"), "")];
        items.extend(accounts.iter().map(|a| {
            let id = a.id.map(|n| n.to_string()).unwrap_or_default();
            PickerItem::person(a.name.clone().unwrap_or_else(|| a.to_string()), "")
                .with_cells(vec![id])
        }));
        let chosen = match self.chosen(account) {
            Some(c) => accounts.iter().position(|a| a.same(c)).map(|i| i + 1),
            None => Some(0),
        };
        let id = match account {
            Account::Owner => "UID",
            Account::Group => "GID",
        };
        let columns = vec![
            PickerColumn { title: t("remote.props_account"), width: 0.0, right: false },
            PickerColumn { title: id.into(), width: 64.0, right: true },
        ];
        self.picker.open_with(items, chosen, Box::new(TableRows::new(columns)));
        self.picking = Some(account);
    }

    /// What the open list said.
    pub(super) fn picked(&mut self, event: Option<PickerEvent>) {
        let Some(event) = event else { return };
        let Some(account) = self.picking.take() else { return };
        let PickerEvent::Chosen(at) = event else { return };
        let picked = at.checked_sub(1).and_then(|i| self.accounts_of(account).get(i).cloned());
        if let WriteAs::Chosen { owner, group } = &mut self.write_as {
            match account {
                Account::Owner => *owner = picked,
                Account::Group => *group = picked,
            }
        }
        self.picker.close();
    }

    /// The project folder's owner or group, as read from the server.
    pub(super) fn project_account(&self, account: Account) -> Option<&Principal> {
        let project = self.server_accounts.as_ref()?.project.as_ref()?;
        match account {
            Account::Owner => project.owner.as_ref(),
            Account::Group => project.group.as_ref(),
        }
    }

    /// The block: its label, the three pills (FTP: why not), then, for the project's folder
    /// and the chosen ones, the owner and the group side by side: read from the folder
    /// (shown, not chosen) or lists to choose from. The pills stay short whatever the names.
    pub(super) fn paint_write_as(
        &mut self,
        pen: Pen<'_, '_>,
        column: Column,
        fonts: &Fonts,
        y: &mut f32,
    ) {
        let Pen { layer, text, icons, ds } = pen;
        let Column { x0, width } = column;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let small = &fonts.small;
        let label = text.shape_line(&t("remote.write_as"), small);
        draw_text(layer, &label, x0, ds::optical_baseline(*y, px(18.0), small), p.text_muted);
        *y += px(26.0);
        if !self.owners_changeable() {
            let note = ds::ellipsize(text, &t("remote.write_as_ftp"), &fonts.ui, width);
            let shaped = text.shape_line(&note, &fonts.ui);
            draw_text(layer, &shaped, x0, ds::optical_baseline(*y, px(28.0), &fonts.ui), p.text);
            *y += px(44.0);
            return;
        }
        let selected = Self::kind_of(&self.write_as);
        let mut x = x0;
        for (i, key) in KINDS.iter().enumerate() {
            let name = t(key);
            let hit = Hit::WriteAs(i);
            let r = ds::choice(
                layer,
                text,
                ds,
                ds::ChoiceSpec {
                    x,
                    mid: *y + px(16.0),
                    label: &name,
                    selected: selected == i,
                    hovered: self.hover == Some(hit),
                },
            );
            self.hits.push((r, hit));
            x = r.max_x() + px(8.0);
        }
        *y += px(44.0);
        if selected == 0 {
            *y += px(4.0);
            return;
        }
        let choosing = selected == 2;
        let gap = px(12.0);
        let half = ((width - gap) / 2.0).floor();
        let loading = self.server_accounts.is_none();
        let reading = t("remote.listing");
        for (i, (account, key)) in
            [(Account::Owner, "remote.props_owner"), (Account::Group, "remote.props_group")]
                .into_iter()
                .enumerate()
        {
            let fx = x0 + i as f32 * (half + gap);
            let title = text.shape_line(&t(key), small);
            draw_text(layer, &title, fx, ds::optical_baseline(*y, px(18.0), small), p.text_muted);
            let r = Rect::new(fx, *y + px(22.0), half, px(34.0));
            let value = if choosing {
                self.chosen(account).map(ToString::to_string)
            } else {
                self.project_account(account)
                    .map(|p| p.name.clone().unwrap_or_else(|| p.to_string()))
            }
            .unwrap_or_default();
            let unset = t("remote.write_as_unset");
            let enabled = choosing && !self.accounts_of(account).is_empty();
            let hit = Hit::Account(account);
            ds::dropdown(
                layer,
                text,
                icons,
                ds,
                r,
                ds::DropdownSpec {
                    value: &value,
                    open: self.picking == Some(account),
                    hovered: self.hover == Some(hit),
                    enabled,
                    note: loading.then_some(reading.as_str()),
                    placeholder: Some(&unset),
                },
            );
            if enabled {
                self.hits.push((r, hit));
                self.account_fields.push((account, r));
            }
        }
        *y += px(22.0) + px(34.0) + px(20.0);
    }
}
