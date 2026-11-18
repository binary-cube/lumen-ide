//! What a console's runner offers on its profiles.

use super::*;

impl Runner {
    /// Profiles `statement` in the background (plan 39h2): the driver's profile
    /// ([`database::Session::profile`]), what's worked out of it, kept in the console's
    /// profiles; its plan shown and told as [`Self::explain`] says.
    pub fn profile(
        &self,
        statement: PlannedStatement,
        options: ProfileOptions,
        connector: Connector,
        source_name: String,
        flavor: sql::Flavor,
        document: Arc<str>,
    ) -> bool {
        if !self.start() {
            return false;
        }
        self.inner.results().replace_unpinned_plans();
        self.inner.clear_diagnostics();
        {
            let mut state = self.inner.state();
            state.connector = Some(Arc::clone(&connector));
            state.source_name = source_name.clone();
            state.snapshot.running =
                Some(Progress { index: 0, total: 1, since: self.inner.runtime.now() });
        }
        self.track(&[statement.sent()], &source_name);
        self.inner.changed();
        let inner = Arc::clone(&self.inner);
        self.inner.runtime.spawn(move || {
            inner.profile(&statement, &options, &connector, &source_name, flavor, &document);
        });
        true
    }

    /// A profile made elsewhere shown as if this console had made it (its tab revealed, its
    /// line in the Output): the snapshots' sample.
    pub fn show_profile(&self, shown: ProfileShown) -> ServiceId {
        self.inner.results().replace_unpinned_plans();
        let plan = shown.report.profile.plan_nodes();
        let id = self.inner.keep_profile(shown, plan);
        self.inner.state().reveal = Some(Some(id));
        self.inner.changed();
        id
    }

    /// What a finding points at lit in the console (`None`: nothing).
    pub fn set_focus(&self, focus: Option<FocusMark>) {
        let changed = {
            let mut state = self.inner.state();
            let changed = state.snapshot.focus != focus;
            state.snapshot.focus = focus;
            changed
        };
        if changed {
            self.inner.changed();
        }
    }

    /// What a finding points at, lit in the console.
    pub fn focus(&self) -> Option<FocusMark> {
        self.inner.state().snapshot.focus.clone()
    }

    /// Rule `rule`'s findings not shown for this console's session (`false`: shown again).
    pub fn ignore_rule(&self, rule: &str, ignored: bool) {
        {
            let mut state = self.inner.state();
            let list = &mut state.snapshot.ignored;
            if ignored && !list.iter().any(|r| r == rule) {
                list.push(rule.to_owned());
            } else if !ignored {
                list.retain(|r| r != rule);
            }
        }
        self.inner.changed();
    }

    /// Every rule shown again.
    pub fn unignore_all(&self) {
        self.inner.state().snapshot.ignored.clear();
        self.inner.changed();
    }

    /// The rules whose findings this console doesn't show.
    pub fn ignored(&self) -> Vec<String> {
        self.inner.state().snapshot.ignored.clone()
    }

    /// Changes when a profile comes, goes or is pinned.
    pub fn profiles_generation(&self) -> u64 {
        self.inner.profiles().generation
    }

    /// Profile `id`, what's worked out of it included (shared).
    pub fn profile_report(&self, id: ProfileId) -> Option<Arc<ProfileReport>> {
        self.inner.profiles().get(id).map(|p| Arc::clone(&p.report))
    }

    /// The profiles kept, oldest first, as "Compare ▾" lists them.
    pub fn profile_list(&self) -> Vec<crate::profile::store::ProfileBrief> {
        self.inner.profiles().iter().map(crate::profile::store::ProfileBrief::of).collect()
    }

    /// What was known of profile `id`'s tables (their statistics once read).
    pub fn profile_facts(&self, id: ProfileId) -> Option<Arc<database::profile::rules::Facts>> {
        self.inner.profiles().get(id).and_then(|p| p.facts.clone())
    }

    /// Profile `id`'s tables' statistics, read in the background if they weren't (once:
    /// `false` when they were asked already or the profile isn't kept).
    pub fn load_table_stats(&self, id: ProfileId) -> bool {
        {
            let mut profiles = self.inner.profiles();
            let Some(p) = profiles.get_mut(id) else { return false };
            if p.stats_asked {
                return false;
            }
            p.stats_asked = true;
        }
        self.queue(Queued::TableStats(id))
    }

    /// The newest profile.
    pub fn latest_profile(&self) -> Option<Arc<ProfileReport>> {
        self.inner.profiles().latest().map(|p| Arc::clone(&p.report))
    }

    /// How many profiles it keeps, and their bytes.
    pub fn profile_memory(&self) -> (usize, usize) {
        let profiles = self.inner.profiles();
        (profiles.len(), profiles.bytes())
    }
}
