use ratatui::widgets::ListState;

pub struct ListSelector<T> {
    pub entries: Vec<T>,
    pub selected: usize,
    pub state: ListState,
}

impl<T> ListSelector<T> {
    pub fn new(entries: Vec<T>) -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Self {
            entries,
            selected: 0,
            state,
        }
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            self.state.select(Some(self.selected));
        }
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
            self.state.select(Some(self.selected));
        }
    }

    pub fn set_entries(&mut self, entries: Vec<T>) {
        self.entries = entries;
        self.selected = 0;
        self.state.select(Some(0));
    }

    pub fn selected_item(&self) -> Option<&T> {
        self.entries.get(self.selected)
    }

    pub fn go_to_top(&mut self) {
        self.selected = 0;
        self.state.select(Some(0));
    }

    pub fn go_to_bottom(&mut self) {
        if !self.entries.is_empty() {
            self.selected = self.entries.len() - 1;
            self.state.select(Some(self.selected));
        }
    }
}
