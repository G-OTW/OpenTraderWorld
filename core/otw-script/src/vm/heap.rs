//! Reference objects (arrays, instances of user-defined types, drawings). A value holds `Value::Ref(id)`; the object lives here, so a
//! session snapshot (a clone of the machine) copies every object with its identity, and a
//! rolled-back forming bar can never have changed the snapshot's arrays.
//!
//! Pine creates a new array each time `array.new*` runs, and most of them die with their bar:
//! the VM collects between bars (`collect`) from its roots (every variable and history slot),
//! once enough objects were allocated since the last collection. Ids are never reused within a
//! run, so a reference kept where the collector does not look (a builtin's own state) reads as
//! a freed array, never as another one.

use std::collections::{BTreeMap, VecDeque};

use super::value::Value;
use crate::output::{BoxOut, Drawings, LabelOut, LineOut, LinefillOut, PolylineOut, Rgba, TableOut};

/// Pine's limit on the elements of one array.
pub const MAX_ARRAY_LEN: usize = 100_000;
/// Live objects at most (scripts that keep every bar's array).
pub const MAX_OBJECTS: usize = 200_000;
/// Weight of every element of every object at most, a string counting one per 24 bytes
/// (about 200 MB): the per-array cap alone lets many large arrays exhaust memory.
pub const MAX_CELLS: usize = 8_000_000;

/// What one element weighs against `MAX_CELLS`.
pub fn weight(v: &Value) -> usize {
    match v {
        Value::Str(s) => 1 + s.len() / 24,
        _ => 1,
    }
}

fn weight_of(o: &Obj) -> usize {
    match o {
        Obj::Array(a) => a.items.iter().map(weight).sum::<usize>() + 1,
        Obj::Object(o) => o.fields.iter().map(weight).sum::<usize>() + 1,
        Obj::Drawing(d) => match &**d {
            Drawing::Table(t) => t.cells.len() + 1,
            Drawing::Polyline(p) => p.points.len() + 1,
            _ => 1,
        },
    }
}

/// Element type of an array, for the results whose type follows it (`array.max` of ints).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Elem {
    Int,
    Float,
    Bool,
    Str,
    Color,
    Any,
}

#[derive(Debug, Clone)]
pub struct Array {
    pub elem: Elem,
    pub items: Vec<Value>,
}

/// An instance of a user-defined type: `ty` indexes the program's type layouts.
#[derive(Debug, Clone)]
pub struct Object {
    pub ty: usize,
    pub fields: Vec<Value>,
}

/// A drawing (phase 5). Unlike arrays it lives until deleted or pushed out by its kind's
/// count limit, whether a variable still holds it or not.
#[derive(Debug, Clone)]
pub enum Drawing {
    Label(LabelOut),
    Line(LineOut),
    Box(BoxOut),
    Table(TableOut),
    /// Two line ids: the linefill goes with either line.
    Linefill { line1: u32, line2: u32, color: Option<Rgba> },
    Polyline(PolylineOut),
}

/// Drawing kinds, in `Heap::order` and limit order.
pub const DRAW_KINDS: [&str; 6] = ["label", "line", "box", "table", "linefill", "polyline"];

impl Drawing {
    pub fn kind(&self) -> usize {
        match self {
            Drawing::Label(_) => 0,
            Drawing::Line(_) => 1,
            Drawing::Box(_) => 2,
            Drawing::Table(_) => 3,
            Drawing::Linefill { .. } => 4,
            Drawing::Polyline(_) => 5,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Obj {
    Array(Array),
    Object(Object),
    Drawing(Box<Drawing>),
}

#[derive(Debug, Clone, Default)]
pub struct Heap {
    objs: BTreeMap<u32, Obj>,
    next: u32,
    since_gc: usize,
    /// Weight of the objects held, collected or not.
    cells: usize,
    /// Live drawing ids per kind (`DRAW_KINDS`), oldest first.
    order: [VecDeque<u32>; 6],
}

impl Heap {
    /// A new object's reference, or `None` past `MAX_OBJECTS` or `MAX_CELLS`.
    pub fn alloc(&mut self, o: Obj) -> Option<Value> {
        if self.objs.len() >= MAX_OBJECTS || !self.grow(weight_of(&o)) {
            return None;
        }
        let id = self.next;
        self.next = self.next.wrapping_add(1);
        self.objs.insert(id, o);
        self.since_gc += 1;
        Some(Value::Ref(id))
    }

    pub fn array(&self, id: u32) -> Option<&Array> {
        match self.objs.get(&id)? {
            Obj::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn array_mut(&mut self, id: u32) -> Option<&mut Array> {
        match self.objs.get_mut(&id)? {
            Obj::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn object(&self, id: u32) -> Option<&Object> {
        match self.objs.get(&id)? {
            Obj::Object(o) => Some(o),
            _ => None,
        }
    }

    pub fn object_mut(&mut self, id: u32) -> Option<&mut Object> {
        match self.objs.get_mut(&id)? {
            Obj::Object(o) => Some(o),
            _ => None,
        }
    }

    pub fn drawing(&self, id: u32) -> Option<&Drawing> {
        match self.objs.get(&id)? {
            Obj::Drawing(d) => Some(d),
            _ => None,
        }
    }

    pub fn drawing_mut(&mut self, id: u32) -> Option<&mut Drawing> {
        match self.objs.get_mut(&id)? {
            Obj::Drawing(d) => Some(d),
            _ => None,
        }
    }

    /// A new drawing; the oldest of its kind is deleted past `limit`. `None` past the object
    /// or cell budget.
    pub fn draw(&mut self, d: Drawing, limit: usize) -> Option<Value> {
        let k = d.kind();
        let v = self.alloc(Obj::Drawing(Box::new(d)))?;
        let Value::Ref(id) = v else { return None };
        self.order[k].push_back(id);
        while self.order[k].len() > limit.max(1) {
            if let Some(old) = self.order[k].front().copied() {
                self.delete(old);
            }
        }
        Some(v)
    }

    /// Delete a drawing (a linefill goes with either of its lines); anything else is ignored.
    pub fn delete(&mut self, id: u32) {
        let Some(Obj::Drawing(d)) = self.objs.get(&id) else { return };
        let k = d.kind();
        self.objs.remove(&id);
        self.order[k].retain(|x| *x != id);
        if k == 1 {
            let fills: Vec<u32> = self.order[4]
                .iter()
                .copied()
                .filter(|f| matches!(self.drawing(*f), Some(Drawing::Linefill { line1, line2, .. }) if *line1 == id || *line2 == id))
                .collect();
            for f in fills {
                self.delete(f);
            }
        }
    }

    /// Live drawings of kind `k` (`DRAW_KINDS` index), oldest first.
    pub fn drawings_of(&self, k: usize) -> impl Iterator<Item = u32> + '_ {
        self.order[k].iter().copied()
    }

    /// Every live drawing, for the run's output.
    pub fn export(&self) -> Drawings {
        let mut out = Drawings::default();
        let mut line_at: BTreeMap<u32, usize> = BTreeMap::new();
        for &id in &self.order[1] {
            if let Some(Drawing::Line(l)) = self.drawing(id) {
                line_at.insert(id, out.lines.len());
                out.lines.push(l.clone());
            }
        }
        for &id in &self.order[0] {
            if let Some(Drawing::Label(l)) = self.drawing(id) {
                out.labels.push(l.clone());
            }
        }
        for &id in &self.order[2] {
            if let Some(Drawing::Box(b)) = self.drawing(id) {
                out.boxes.push(b.clone());
            }
        }
        for &id in &self.order[3] {
            if let Some(Drawing::Table(t)) = self.drawing(id) {
                out.tables.push(t.clone());
            }
        }
        for &id in &self.order[4] {
            if let Some(Drawing::Linefill { line1, line2, color }) = self.drawing(id) {
                if let (Some(&a), Some(&b)) = (line_at.get(line1), line_at.get(line2)) {
                    out.linefills.push(LinefillOut { line1: a, line2: b, color: *color });
                }
            }
        }
        for &id in &self.order[5] {
            if let Some(Drawing::Polyline(p)) = self.drawing(id) {
                out.polylines.push(p.clone());
            }
        }
        out
    }

    pub fn len(&self) -> usize {
        self.objs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objs.is_empty()
    }

    /// Count `n` more cells (an element added to an array); `false` past `MAX_CELLS`.
    pub fn grow(&mut self, n: usize) -> bool {
        if self.cells.saturating_add(n) > MAX_CELLS {
            return false;
        }
        self.cells += n;
        true
    }

    /// Worth collecting: more new objects since the last collection than were alive then, or
    /// half the cell budget used.
    pub fn wants_collect(&self) -> bool {
        self.since_gc >= 4096.max(self.objs.len() / 2) || self.cells > MAX_CELLS / 2
    }

    /// Keep the objects reachable from `roots` (and from each other), drop the rest.
    pub fn collect<'v>(&mut self, roots: impl Iterator<Item = &'v Value>) {
        let mut marked = std::collections::BTreeSet::new();
        // Drawings stay until deleted: they are roots themselves.
        let mut work: Vec<u32> = self.order.iter().flatten().copied().collect();
        for v in roots {
            refs_in(v, &mut work);
        }
        while let Some(id) = work.pop() {
            if !marked.insert(id) {
                continue;
            }
            let items = match self.objs.get(&id) {
                Some(Obj::Array(a)) => &a.items,
                Some(Obj::Object(o)) => &o.fields,
                Some(Obj::Drawing(_)) | None => continue,
            };
            for v in items {
                refs_in(v, &mut work);
            }
        }
        self.objs.retain(|id, _| marked.contains(id));
        self.since_gc = 0;
        self.cells = self.objs.values().map(weight_of).sum();
    }
}

fn refs_in(v: &Value, out: &mut Vec<u32>) {
    match v {
        Value::Ref(id) => out.push(*id),
        Value::Tuple(items) => items.iter().for_each(|i| refs_in(i, out)),
        _ => {}
    }
}
