use super::*;

impl App {
    pub(super) fn open_sort_picker(&mut self) -> Reaction {
        let cursor = Field::all()
            .position(|field| field == self.table.sort.field)
            .unwrap_or(0);
        self.screen = Screen::SortPicker { cursor };
        Reaction::Continue
    }

    pub(super) fn open_user_picker(&mut self) -> Reaction {
        let mut ids: Vec<_> = self.table.rows.iter().map(|row| row.uid).collect();
        ids.sort_unstable();
        ids.dedup();
        let mut owners = vec![(None, "All users".to_string())];
        owners.extend(
            ids.into_iter()
                .map(|uid| (Some(uid), format!("{} ({uid})", self.users.name(uid)))),
        );
        let cursor = owners
            .iter()
            .position(|entry| entry.0 == self.user_filter)
            .unwrap_or(0);
        self.screen = Screen::UserPicker {
            cursor,
            query: String::new(),
            owners,
        };
        Reaction::Continue
    }

    pub(super) fn handle_picker_key(&mut self, key: Key) -> Reaction {
        let current = std::mem::replace(&mut self.screen, Screen::ProcessList);
        match current {
            Screen::SortPicker { mut cursor } => {
                let fields: Vec<_> = Field::all().collect();
                match key {
                    Key::Esc | Key::F(10) => return Reaction::Continue,
                    Key::Enter => {
                        if let Some(&field) = fields.get(cursor) {
                            self.choose_sort(field, false);
                        }
                        return Reaction::Reorder;
                    }
                    Key::Up => cursor = cursor.saturating_sub(1),
                    Key::Down => cursor = (cursor + 1).min(fields.len().saturating_sub(1)),
                    Key::Home => cursor = 0,
                    Key::End => cursor = fields.len().saturating_sub(1),
                    _ => {}
                }
                self.screen = Screen::SortPicker { cursor };
            }
            Screen::UserPicker {
                mut cursor,
                mut query,
                owners,
            } => {
                match key {
                    Key::Esc | Key::F(10) => return Reaction::Continue,
                    Key::Enter => {
                        if let Some(&(uid, _)) = filtered_owners(&owners, &query).get(cursor) {
                            self.user_filter = *uid;
                        }
                        return Reaction::Reorder;
                    }
                    Key::Char(ch) => {
                        query.push(ch);
                        cursor = 0;
                    }
                    Key::Backspace => {
                        query.pop();
                        cursor = 0;
                    }
                    Key::Up => cursor = cursor.saturating_sub(1),
                    Key::Down => {
                        cursor = (cursor + 1)
                            .min(filtered_owners(&owners, &query).len().saturating_sub(1))
                    }
                    Key::Home => cursor = 0,
                    Key::End => cursor = filtered_owners(&owners, &query).len().saturating_sub(1),
                    _ => {}
                }
                self.screen = Screen::UserPicker {
                    cursor,
                    query,
                    owners,
                };
            }
            other => self.screen = other,
        }
        Reaction::Continue
    }

    pub(super) fn choose_sort(&mut self, field: Field, invert_active: bool) {
        if invert_active && field == self.table.sort.field {
            self.table.sort.dir = if self.table.sort.dir == Dir::Asc {
                Dir::Desc
            } else {
                Dir::Asc
            };
        } else {
            self.table.sort.field = field;
            self.table.sort.dir = if field.default_sort_desc() {
                Dir::Desc
            } else {
                Dir::Asc
            };
        }
        self.sync_sort_to_tab();
    }

    pub fn click_header(&mut self, x: u16, y: u16) {
        if self.screen != Screen::ProcessList || self.input.active {
            return;
        }
        if let Some(field) = self
            .header_regions
            .iter()
            .find_map(|(rect, field)| rect.contains((x, y).into()).then_some(*field))
        {
            self.choose_sort(field, true);
            self.reorder();
        }
    }

    pub(super) fn fold_selected(&mut self, expand: bool) -> Reaction {
        if let Some(row) = self.selected_row() {
            let identity = (row.identity(), row.starttime);
            if row.has_children {
                if expand {
                    self.collapsed.remove(&identity);
                } else {
                    self.collapsed.insert(identity);
                }
            }
        }
        Reaction::Reorder
    }

    pub(super) fn fold_all(&mut self) -> Reaction {
        let expandable: Vec<_> = self
            .table
            .rows
            .iter()
            .filter(|row| row.has_children)
            .map(|row| (row.identity(), row.starttime))
            .collect();
        if expandable
            .iter()
            .any(|identity| !self.collapsed.contains(identity))
        {
            self.collapsed.extend(expandable);
        } else {
            self.collapsed.clear();
        }
        Reaction::Reorder
    }

    pub(super) fn apply_folds(&mut self) {
        if !self.table.tree_mode {
            return;
        }
        let mut hidden_below = None;
        self.table.view.order.retain(|&index| {
            let row = &self.table.rows[index];
            if hidden_below.is_some_and(|depth| row.depth > depth) {
                return false;
            }
            hidden_below = if self.collapsed.contains(&(row.identity(), row.starttime)) {
                Some(row.depth)
            } else {
                None
            };
            true
        });
    }

    pub(super) fn restore_selection(&mut self, identity: Option<(u64, u64)>) {
        let Some(identity) = identity else {
            return;
        };
        let mut target = self
            .table
            .rows
            .iter()
            .position(|row| (row.identity(), row.starttime) == identity);
        for _ in 0..self.table.rows.len() {
            let Some(index) = target else {
                break;
            };
            if let Some(position) = self.table.view.order.iter().position(|&i| i == index) {
                self.selected = position;
                return;
            }
            if !self.table.tree_mode {
                break;
            }
            let row = &self.table.rows[index];
            let parent = if row.is_thread { row.tgid } else { row.ppid };
            target = self
                .table
                .rows
                .iter()
                .position(|row| !row.is_thread && row.pid == parent);
        }
    }

    pub(super) fn expand_search_path(&mut self, needle: &str) {
        if !self.table.tree_mode || self.collapsed.is_empty() {
            return;
        }
        let full = tree::build(&mut self.table.rows);
        if let Some(position) = table::find_next(&self.table.rows, &full, 0, needle) {
            let mut depth = self.table.rows[full[position]].depth;
            for &index in full[..position].iter().rev() {
                let row = &self.table.rows[index];
                if row.depth < depth {
                    self.collapsed.remove(&(row.identity(), row.starttime));
                    depth = row.depth;
                }
            }
            self.reorder();
        }
    }

    pub(super) fn filter_area(&self, area: Rect, buf: &mut Buffer) -> Rect {
        if self.filter.is_none() && self.user_filter.is_none() {
            return area;
        }
        let mut eligible = self.visibility.clone();
        eligible.text = None;
        eligible.uid = None;
        let total = self
            .table
            .rows
            .iter()
            .filter(|row| eligible.allows(row))
            .count();
        let matched = self
            .table
            .rows
            .iter()
            .filter(|row| self.visibility.allows(row))
            .count();
        let mut conditions = vec![format!("{matched}/{total}")];
        if let Some(filter) = &self.filter {
            conditions.push(format!("Filter: {}", filter.expression()));
        }
        if let Some(uid) = self.user_filter {
            conditions.push(format!("User: {uid}"));
        }
        let lines = crate::command::wrap(&conditions.join("  "), area.width as usize);
        let height = (lines.len() as u16).min(area.height.saturating_sub(2));
        Widget::render(
            Paragraph::new(
                lines
                    .into_iter()
                    .take(height as usize)
                    .map(Line::from)
                    .collect::<Vec<_>>(),
            )
            .style(self.theme.meter_label),
            Rect::new(area.x, area.y, area.width, height),
            buf,
        );
        Rect::new(
            area.x,
            area.y + height,
            area.width,
            area.height.saturating_sub(height),
        )
    }

    pub(super) fn draw_secondary(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);
        self.draw_tabs(chunks[0], frame.buffer_mut());
        let content = chunks[1];
        self.info_viewport = content.height.saturating_sub(2) as usize;
        if let Some(source) = &self.command_source {
            let width = content.width.saturating_sub(2);
            if width != self.command_width {
                if let Screen::Info { lines, scroll, .. } = &mut self.screen {
                    let anchor: usize = lines.iter().take(*scroll).map(String::len).sum();
                    *lines = crate::command::wrap(source, width as usize);
                    let mut offset = 0;
                    *scroll = lines
                        .iter()
                        .position(|line| {
                            offset += line.len();
                            offset > anchor
                        })
                        .unwrap_or(0);
                }
                self.command_width = width;
            }
        }
        if let Screen::Info { lines, scroll, .. } = &mut self.screen {
            *scroll = (*scroll).min(lines.len().saturating_sub(self.info_viewport.max(1)));
        }
        match &self.screen {
            Screen::Help => draw_help(content, frame.buffer_mut(), &self.theme),
            Screen::Info {
                title,
                lines,
                scroll,
            } => draw_info(
                content,
                frame.buffer_mut(),
                &self.theme,
                title,
                lines,
                *scroll,
            ),
            Screen::Setup => {
                if let Some(setup) = &self.setup {
                    setup.draw(content, frame.buffer_mut(), &self.theme);
                }
            }
            Screen::Affinity(panel) => panel.draw(content, frame.buffer_mut(), &self.theme),
            Screen::IoPriority(panel) => panel.draw(content, frame.buffer_mut(), &self.theme),
            Screen::SignalPicker { cursor } => draw_signal_picker(
                content,
                frame.buffer_mut(),
                &self.theme,
                &self.action_targets,
                *cursor,
            ),
            Screen::SortPicker { cursor } => {
                let labels = Field::all()
                    .map(|field| {
                        format!(
                            "{}  {}",
                            field.title().trim(),
                            field.meta().map_or("", |meta| meta.description)
                        )
                    })
                    .collect::<Vec<_>>();
                draw_choices(
                    content,
                    frame.buffer_mut(),
                    &self.theme,
                    "Sort by",
                    &labels,
                    *cursor,
                );
            }
            Screen::UserPicker {
                cursor,
                query,
                owners,
            } => {
                let labels = filtered_owners(owners, query)
                    .iter()
                    .map(|(_, label)| (*label).clone())
                    .collect::<Vec<_>>();
                draw_choices(
                    content,
                    frame.buffer_mut(),
                    &self.theme,
                    &format!("User: {query}"),
                    &labels,
                    *cursor,
                );
            }
            Screen::ProcessList => {}
        }
        self.draw_function_bar(chunks[2], frame.buffer_mut());
    }
}

fn filtered_owners<'a>(
    owners: &'a [(Option<u32>, String)],
    query: &str,
) -> Vec<(&'a Option<u32>, &'a String)> {
    let query = query.to_lowercase();
    owners
        .iter()
        .filter(|(_, label)| label.to_lowercase().contains(&query))
        .map(|(uid, label)| (uid, label))
        .collect()
}

fn draw_choices(
    area: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    title: &str,
    labels: &[String],
    cursor: usize,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(span_title(title, theme))
        .border_style(theme.dialog_border);
    let inner = block.inner(area);
    Widget::render(block, area, buf);
    let offset = cursor.saturating_sub(inner.height.saturating_sub(1) as usize);
    let lines = labels
        .iter()
        .enumerate()
        .skip(offset)
        .take(inner.height as usize)
        .map(|(index, label)| {
            Line::styled(
                label.clone(),
                if index == cursor {
                    theme.panel_selected
                } else {
                    theme.row_normal
                },
            )
        })
        .collect::<Vec<_>>();
    Widget::render(Paragraph::new(lines), inner, buf);
}
