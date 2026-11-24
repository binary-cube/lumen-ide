//! The owner's and the group's lists (the server's users and groups, `name [id]`) and the
//! checksum's algorithms: what each offers, what's shown chosen, what may change, a pick.

use super::*;

impl PropertiesView {
    /// The owner or the group as all the chosen have it (none chosen in the dialog).
    fn shared_of(&self, choice: Choice) -> Option<Principal> {
        let of = |p: &Properties| match choice {
            Choice::Owner => p.owner.clone(),
            Choice::Group => p.group.clone(),
        };
        shared(self.props().iter().map(of)).flatten()
    }

    /// The id chosen for the owner or the group, when it differs from what they have.
    pub(super) fn chosen_id(&self, choice: Choice) -> Option<u32> {
        let chosen = match choice {
            Choice::Owner => self.owner.as_ref(),
            Choice::Group => self.group.as_ref(),
        }?;
        let unchanged = self.shared_of(choice).is_some_and(|s| s.same(chosen));
        if unchanged { None } else { chosen.id }
    }

    /// The accounts a list offers.
    pub(super) fn accounts(&self, choice: Choice) -> &[Principal] {
        let Some(loaded) = &self.loaded else { return &[] };
        match choice {
            Choice::Owner => &loaded.accounts.users,
            Choice::Group => &loaded.accounts.groups,
        }
    }

    /// A list's rows (each account: its name, its id) and the one shown.
    pub(super) fn options(&self, choice: Choice) -> (Vec<PickerItem>, Option<usize>) {
        let shown = self.value_of(choice);
        let accounts = self.accounts(choice);
        let items = accounts
            .iter()
            .map(|a| {
                let id = a.id.map(|n| n.to_string()).unwrap_or_default();
                let name = a.name.clone().unwrap_or_else(|| a.to_string());
                PickerItem::person(name, "").with_cells(vec![id])
            })
            .collect();
        let chosen = shown.and_then(|s| accounts.iter().position(|a| a.same(&s)));
        (items, chosen)
    }

    /// A list opened from its field.
    pub(super) fn open_list(&mut self, choice: Choice) {
        let (items, chosen) = self.options(choice);
        // A table: the name, the id in a column of its own (titled, fixed while scrolling).
        let id = match choice {
            Choice::Owner => "UID",
            Choice::Group => "GID",
        };
        let columns = vec![
            PickerColumn { title: t("remote.props_account"), width: 0.0, right: false },
            PickerColumn { title: id.into(), width: 64.0, right: true },
        ];
        self.picker.open_with(items, chosen, Box::new(TableRows::new(columns)));
        self.picking = Some(Pick::Account(choice));
        self.focus_list();
    }

    /// The algorithms' list opened.
    pub(super) fn open_algos(&mut self) {
        let items = Algo::ALL
            .iter()
            .map(|a| PickerItem { label: a.label().into(), ..PickerItem::default() })
            .collect();
        let chosen = Algo::ALL.iter().position(|a| *a == self.sums.algo());
        self.picker.open(items, chosen);
        self.picking = Some(Pick::Algo);
        self.focus_list();
    }

    /// What the open list said.
    pub(super) fn picked(&mut self, event: Option<PickerEvent>) {
        match (event, self.picking) {
            (Some(PickerEvent::Chosen(at)), Some(Pick::Account(choice))) => self.choose(choice, at),
            (Some(PickerEvent::Chosen(at)), Some(Pick::Algo)) => {
                if let Some(algo) = Algo::ALL.get(at) {
                    self.sums.choose(*algo);
                }
                self.picking = None;
            }
            (Some(_), _) => self.picking = None,
            (None, _) => {}
        }
    }

    /// The owner or the group shown: the one chosen, else the one all have (`None`: they
    /// differ, or the server doesn't say).
    fn value_of(&self, choice: Choice) -> Option<Principal> {
        let chosen = match choice {
            Choice::Owner => &self.owner,
            Choice::Group => &self.group,
        };
        chosen.clone().or_else(|| {
            let of = |p: &Properties| match choice {
                Choice::Owner => p.owner.clone(),
                Choice::Group => p.group.clone(),
            };
            shared(self.props().iter().map(of)).flatten()
        })
    }

    /// What a list's field says.
    pub(super) fn label_of(&self, choice: Choice) -> String {
        if let Some(value) = self.value_of(choice) {
            return value.to_string();
        }
        let known = self.props().iter().any(|p| match choice {
            Choice::Owner => p.owner.is_some(),
            Choice::Group => p.group.is_some(),
        });
        if known { t("remote.props_mixed") } else { "—".into() }
    }

    /// The server lets it change (this protocol, this session).
    pub(super) fn allowed(&self, choice: Choice) -> bool {
        self.loaded.as_ref().is_some_and(|l| match choice {
            Choice::Owner => l.caps.chown,
            Choice::Group => l.caps.chgrp,
        })
    }

    pub(super) fn may_change(&self, choice: Choice) -> bool {
        self.allowed(choice) && !self.accounts(choice).is_empty()
    }

    pub(super) fn choose(&mut self, choice: Choice, at: usize) {
        let picked = self.accounts(choice).get(at).cloned();
        match choice {
            Choice::Owner => self.owner = picked,
            Choice::Group => self.group = picked,
        }
        self.picking = None;
        self.picker.close();
    }
}
