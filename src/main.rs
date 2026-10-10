//! arvore — a terminal playground for learning tree data structures.
//!
//! One file, in order:
//!   1. model     – drawable `Node`, recorded `Step`, the `Tree` trait
//!   2. trees     – BST / AVL / Red-Black (one arena), Heap, Trie, B-Tree
//!   3. layout    – turns a `Node` into a box-drawing character grid
//!   4. lessons   – the text shown in the Lesson pane
//!   5. app (TUI) – ratatui front end with a replayable timeline
//!   6. main      – `arvore` (TUI), `arvore show`, `arvore ls`

use std::collections::{BTreeMap, VecDeque};
use std::io::{self, IsTerminal};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};

// ───────────────────────────── 1. model ─────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
enum Tone {
    Plain,
    Red,
    Black,
    End,
}

/// A drawable tree node. `None` kids keep left/right position in binary trees.
#[derive(Clone)]
struct Node {
    label: String,
    tone: Tone,
    hot: bool,
    kids: Vec<Option<Node>>,
}

/// One frame of the timeline: the whole tree plus what just happened.
#[derive(Clone)]
struct Step {
    root: Option<Node>,
    msg: String,
    aux: String,
}

type Res = Result<Vec<Step>, String>;
type Page = (&'static str, &'static str);

trait Tree {
    fn name(&self) -> &'static str;
    fn blurb(&self) -> &'static str;
    fn lesson(&self) -> &'static [Page];
    fn insert(&mut self, tok: &str) -> Res;
    fn delete(&mut self, tok: &str) -> Res;
    fn find(&mut self, tok: &str) -> Res;
    /// Structure-specific commands (`pop` for heaps, `complete` for tries).
    fn special(&mut self, _cmd: &str, _args: &[&str]) -> Option<Res> {
        None
    }
    fn snapshot(&self, msg: &str) -> Step;
    fn clear(&mut self);
    fn random_token(&self, rng: &mut Rng) -> String;
}

struct Rng(u64);

impl Rng {
    fn new() -> Self {
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(88172645463325252);
        Rng(t | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn range(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn int(tok: &str) -> Result<i64, String> {
    tok.parse::<i64>().map_err(|_| format!("'{tok}' is not an integer"))
}

// ───────────────────── 2a. BST / AVL / Red-Black ─────────────────────

const NIL: usize = 0;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Bst,
    Avl,
    Rb,
}

struct N {
    key: i64,
    l: usize,
    r: usize,
    p: usize,
    red: bool,
    h: i32,
}

/// Arena-based binary search tree. Index 0 is the shared NIL sentinel
/// (black, height 0), which keeps the red-black algorithms textbook-shaped.
struct BinTree {
    kind: Kind,
    n: Vec<N>,
    root: usize,
    free: Vec<usize>,
    out: Vec<Step>,
}

impl BinTree {
    fn new(kind: Kind) -> Self {
        let nil = N { key: 0, l: NIL, r: NIL, p: NIL, red: false, h: 0 };
        BinTree { kind, n: vec![nil], root: NIL, free: vec![], out: vec![] }
    }

    fn key(&self, i: usize) -> i64 {
        self.n[i].key
    }
    fn red(&self, i: usize) -> bool {
        self.n[i].red
    }
    fn ch(&self, i: usize, left: bool) -> usize {
        if left { self.n[i].l } else { self.n[i].r }
    }
    fn bf(&self, i: usize) -> i32 {
        self.n[self.n[i].l].h - self.n[self.n[i].r].h
    }
    fn upd(&mut self, i: usize) {
        if i != NIL {
            self.n[i].h = 1 + self.n[self.n[i].l].h.max(self.n[self.n[i].r].h);
        }
    }

    fn alloc(&mut self, key: i64, p: usize) -> usize {
        let node = N { key, l: NIL, r: NIL, p, red: true, h: 1 };
        if let Some(i) = self.free.pop() {
            self.n[i] = node;
            i
        } else {
            self.n.push(node);
            self.n.len() - 1
        }
    }

    fn build(&self, i: usize, hot: &[usize]) -> Option<Node> {
        if i == NIL {
            return None;
        }
        let x = &self.n[i];
        let (label, tone) = match self.kind {
            Kind::Bst => (x.key.to_string(), Tone::Plain),
            Kind::Avl => {
                let bf = match self.bf(i) {
                    0 => "0".to_string(),
                    b => format!("{b:+}"),
                };
                (format!("{} {}", x.key, bf), Tone::Plain)
            }
            Kind::Rb => (x.key.to_string(), if x.red { Tone::Red } else { Tone::Black }),
        };
        Some(Node {
            label,
            tone,
            hot: hot.contains(&i),
            kids: vec![self.build(x.l, hot), self.build(x.r, hot)],
        })
    }

    fn snap(&self, msg: String, hot: &[usize]) -> Step {
        Step { root: self.build(self.root, hot), msg, aux: String::new() }
    }

    fn rec(&mut self, msg: impl Into<String>, hot: &[usize]) {
        let s = self.snap(msg.into(), hot);
        self.out.push(s);
    }

    fn run(&mut self, f: impl FnOnce(&mut Self) -> Result<(), String>) -> Res {
        self.out.clear();
        let r = f(self);
        let out = std::mem::take(&mut self.out);
        r.map(|_| out)
    }

    fn transplant(&mut self, u: usize, v: usize) {
        let p = self.n[u].p;
        if p == NIL {
            self.root = v;
        } else if u == self.n[p].l {
            self.n[p].l = v;
        } else {
            self.n[p].r = v;
        }
        self.n[v].p = p;
    }

    fn rot_left(&mut self, x: usize) {
        let y = self.n[x].r;
        let yl = self.n[y].l;
        self.n[x].r = yl;
        if yl != NIL {
            self.n[yl].p = x;
        }
        self.transplant(x, y);
        self.n[y].l = x;
        self.n[x].p = y;
        self.upd(x);
        self.upd(y);
    }

    fn rot_right(&mut self, x: usize) {
        let y = self.n[x].l;
        let yr = self.n[y].r;
        self.n[x].l = yr;
        if yr != NIL {
            self.n[yr].p = x;
        }
        self.transplant(x, y);
        self.n[y].r = x;
        self.n[x].p = y;
        self.upd(x);
        self.upd(y);
    }

    fn rot(&mut self, i: usize, left: bool) {
        if left { self.rot_left(i) } else { self.rot_right(i) }
    }

    // ---- insert ----

    fn do_insert(&mut self, k: i64) -> Result<(), String> {
        let (mut y, mut x) = (NIL, self.root);
        if x == NIL {
            self.rec("tree is empty: the new key becomes the root", &[]);
        }
        while x != NIL {
            let key = self.key(x);
            if k == key {
                return Err(format!("{k} is already in the tree"));
            }
            let left = k < key;
            self.rec(
                format!("{k} {} {key}: go {}", if left { "<" } else { ">" }, if left { "left" } else { "right" }),
                &[x],
            );
            y = x;
            x = self.ch(x, left);
        }
        let z = self.alloc(k, y);
        let msg = if y == NIL {
            self.root = z;
            format!("insert {k} as the root")
        } else if k < self.key(y) {
            self.n[y].l = z;
            format!("insert {k} as the left child of {}", self.key(y))
        } else {
            self.n[y].r = z;
            format!("insert {k} as the right child of {}", self.key(y))
        };
        self.rec(msg, &[z]);
        match self.kind {
            Kind::Bst => {}
            Kind::Avl => self.avl_fix(y),
            Kind::Rb => self.rb_insert_fix(z),
        }
        Ok(())
    }

    fn avl_fix(&mut self, from: usize) {
        let mut x = from;
        while x != NIL {
            self.upd(x);
            if self.bf(x).abs() > 1 {
                x = self.rebalance(x);
            }
            x = self.n[x].p;
        }
    }

    /// Fixes the unbalanced node `x`; returns the new root of that subtree.
    fn rebalance(&mut self, x: usize) -> usize {
        let kx = self.key(x);
        let bf = self.bf(x);
        let left_heavy = bf > 0;
        let c = self.ch(x, left_heavy);
        let inner_heavy = if left_heavy { self.bf(c) < 0 } else { self.bf(c) > 0 };
        if inner_heavy {
            let kc = self.key(c);
            let case = if left_heavy { "left-right" } else { "right-left" };
            self.rec(
                format!("{kx} has balance {bf:+}: {case} case — first rotate {} at {kc}", if left_heavy { "left" } else { "right" }),
                &[x, c],
            );
            self.rot(c, left_heavy);
            let nc = self.ch(x, left_heavy);
            self.rec(format!("now it is a straight line; rotate {} at {kx}", if left_heavy { "right" } else { "left" }), &[x, nc]);
        } else {
            let case = if left_heavy { "left-left" } else { "right-right" };
            self.rec(
                format!("{kx} has balance {bf:+}: {case} case — rotate {} at {kx}", if left_heavy { "right" } else { "left" }),
                &[x, c],
            );
        }
        self.rot(x, !left_heavy);
        let top = self.n[x].p;
        let kt = self.key(top);
        self.rec(format!("balanced: {kt} is the new subtree root"), &[top]);
        top
    }

    fn rb_insert_fix(&mut self, mut z: usize) {
        while self.red(self.n[z].p) {
            let mut p = self.n[z].p;
            let g = self.n[p].p;
            let left = p == self.n[g].l;
            let u = self.ch(g, !left);
            if self.red(u) {
                let (kp, ku, kg) = (self.key(p), self.key(u), self.key(g));
                self.rec(format!("parent {kp} and uncle {ku} are red: recolor them black, grandparent {kg} red"), &[z, p, u, g]);
                self.n[p].red = false;
                self.n[u].red = false;
                self.n[g].red = true;
                self.rec(format!("recolored; check {kg} against its parent"), &[g]);
                z = g;
            } else {
                if z == self.ch(p, !left) {
                    let (kz, kp) = (self.key(z), self.key(p));
                    self.rec(format!("uncle is black and {kz} is an inner child: rotate {} at {kp}", if left { "left" } else { "right" }), &[z, p]);
                    z = p;
                    self.rot(z, left);
                    self.rec("now the red-red pair is a straight line", &[z]);
                }
                p = self.n[z].p;
                let g = self.n[p].p;
                let (kp, kg) = (self.key(p), self.key(g));
                self.rec(
                    format!("uncle is black: recolor {kp} black, {kg} red, then rotate {} at {kg}", if left { "right" } else { "left" }),
                    &[p, g],
                );
                self.n[p].red = false;
                self.n[g].red = true;
                self.rot(g, !left);
                self.rec(format!("fixed: {kp} is the subtree root"), &[p]);
                break;
            }
        }
        if self.n[self.root].red {
            let r = self.root;
            self.n[r].red = false;
            self.rec("the root is always black", &[r]);
        }
    }

    // ---- delete ----

    fn do_delete(&mut self, k: i64) -> Result<(), String> {
        let mut z = self.root;
        while z != NIL {
            let key = self.key(z);
            if k == key {
                break;
            }
            let left = k < key;
            self.rec(format!("{k} {} {key}: go {}", if left { "<" } else { ">" }, if left { "left" } else { "right" }), &[z]);
            z = self.ch(z, left);
        }
        if z == NIL {
            return Err(format!("{k} is not in the tree"));
        }
        let (zl, zr) = (self.n[z].l, self.n[z].r);
        let why = match (zl == NIL, zr == NIL) {
            (true, true) => "it is a leaf: just remove it".to_string(),
            (false, false) => "two children: replace it with its in-order successor".to_string(),
            _ => "one child: lift the child into its place".to_string(),
        };
        self.rec(format!("found {k}: {why}"), &[z]);

        let mut y_was_red = self.red(z);
        let (x, fix_parent);
        if zl == NIL {
            x = zr;
            fix_parent = self.n[z].p;
            self.transplant(z, zr);
        } else if zr == NIL {
            x = zl;
            fix_parent = self.n[z].p;
            self.transplant(z, zl);
        } else {
            let mut y = zr;
            while self.n[y].l != NIL {
                y = self.n[y].l;
            }
            y_was_red = self.red(y);
            x = self.n[y].r;
            if self.n[y].p == z {
                self.n[x].p = y;
                fix_parent = y;
            } else {
                fix_parent = self.n[y].p;
                self.transplant(y, x);
                let r = self.n[z].r;
                self.n[y].r = r;
                self.n[r].p = y;
            }
            self.transplant(z, y);
            let l = self.n[z].l;
            self.n[y].l = l;
            self.n[l].p = y;
            self.n[y].red = self.red(z);
            self.upd(y);
        }
        self.free.push(z);
        let hot: Vec<usize> = if x != NIL { vec![x] } else { vec![] };
        self.rec(format!("removed {k}"), &hot);

        match self.kind {
            Kind::Bst => {}
            Kind::Avl => self.avl_fix(fix_parent),
            Kind::Rb => {
                if !y_was_red {
                    self.rec("a black node left the tree: its path is one black short (double black)", &hot);
                    self.rb_delete_fix(x);
                }
            }
        }
        Ok(())
    }

    fn rb_delete_fix(&mut self, mut x: usize) {
        while x != self.root && !self.red(x) {
            let p = self.n[x].p;
            let left = x == self.n[p].l;
            let mut w = self.ch(p, !left);
            let side = if left { "left" } else { "right" };
            let other = if left { "right" } else { "left" };
            if self.red(w) {
                let kp = self.key(p);
                self.rec(format!("sibling is red: recolor, then rotate {side} at {kp}"), &[p, w]);
                self.n[w].red = false;
                self.n[p].red = true;
                self.rot(p, left);
                w = self.ch(p, !left);
                self.rec("sibling is black now", &[w]);
            }
            let (near, far) = (self.ch(w, left), self.ch(w, !left));
            if !self.red(near) && !self.red(far) {
                let kw = self.key(w);
                self.rec(format!("sibling {kw} and both its children are black: recolor sibling red, push the problem up"), &[w, p]);
                self.n[w].red = true;
                x = p;
            } else {
                if !self.red(far) {
                    let kw = self.key(w);
                    self.rec(format!("sibling's far child is black: recolor, rotate {other} at {kw}"), &[w, near]);
                    self.n[near].red = false;
                    self.n[w].red = true;
                    self.rot(w, !left);
                    w = self.ch(p, !left);
                }
                let (kp, kw) = (self.key(p), self.key(w));
                self.rec(format!("sibling's far child is red: copy {kp}'s color to {kw}, recolor, rotate {side} at {kp}"), &[p, w]);
                self.n[w].red = self.red(p);
                self.n[p].red = false;
                let far = self.ch(w, !left);
                self.n[far].red = false;
                self.rot(p, left);
                self.rec("black height restored", &[w]);
                x = self.root;
            }
        }
        if x != NIL && self.red(x) {
            self.rec("absorb the extra black into the red node", &[x]);
        }
        self.n[x].red = false;
        if x != NIL {
            self.rec("fixed", &[x]);
        }
    }

    fn do_find(&mut self, k: i64) {
        let mut x = self.root;
        if x == NIL {
            self.rec("the tree is empty", &[]);
        }
        while x != NIL {
            let key = self.key(x);
            if k == key {
                self.rec(format!("found {k}"), &[x]);
                return;
            }
            let left = k < key;
            self.rec(format!("{k} {} {key}: go {}", if left { "<" } else { ">" }, if left { "left" } else { "right" }), &[x]);
            x = self.ch(x, left);
        }
        self.rec(format!("reached an empty slot: {k} is not in the tree"), &[]);
    }

    #[cfg(test)]
    fn check(&self) -> usize {
        fn go(t: &BinTree, i: usize, lo: Option<i64>, hi: Option<i64>, count: &mut usize) -> (i32, i32) {
            if i == NIL {
                return (0, 1);
            }
            *count += 1;
            let k = t.key(i);
            assert!(lo.map_or(true, |l| k > l) && hi.map_or(true, |h| k < h), "order violated at {k}");
            for c in [t.n[i].l, t.n[i].r] {
                if c != NIL {
                    assert_eq!(t.n[c].p, i, "parent pointer of {}", t.key(c));
                }
            }
            let (hl, bl) = go(t, t.n[i].l, lo, Some(k), count);
            let (hr, br) = go(t, t.n[i].r, Some(k), hi, count);
            let h = 1 + hl.max(hr);
            if t.kind == Kind::Avl {
                assert!((hl - hr).abs() <= 1, "unbalanced at {k}");
                assert_eq!(t.n[i].h, h, "stale height at {k}");
            }
            if t.kind == Kind::Rb {
                assert_eq!(bl, br, "black height differs at {k}");
                if t.red(i) {
                    assert!(!t.red(t.n[i].l) && !t.red(t.n[i].r), "red-red at {k}");
                }
                return (h, bl + if t.red(i) { 0 } else { 1 });
            }
            (h, 0)
        }
        if self.root != NIL {
            assert_eq!(self.n[self.root].p, NIL);
            if self.kind == Kind::Rb {
                assert!(!self.red(self.root), "red root");
            }
        }
        let mut count = 0;
        go(self, self.root, None, None, &mut count);
        count
    }
}

impl Tree for BinTree {
    fn name(&self) -> &'static str {
        match self.kind {
            Kind::Bst => "BST",
            Kind::Avl => "AVL",
            Kind::Rb => "Red-Black",
        }
    }
    fn blurb(&self) -> &'static str {
        match self.kind {
            Kind::Bst => "ordered, unbalanced",
            Kind::Avl => "strictly balanced",
            Kind::Rb => "loosely balanced",
        }
    }
    fn lesson(&self) -> &'static [Page] {
        match self.kind {
            Kind::Bst => LESSON_BST,
            Kind::Avl => LESSON_AVL,
            Kind::Rb => LESSON_RB,
        }
    }
    fn insert(&mut self, tok: &str) -> Res {
        let k = int(tok)?;
        self.run(|t| t.do_insert(k))
    }
    fn delete(&mut self, tok: &str) -> Res {
        let k = int(tok)?;
        self.run(|t| t.do_delete(k))
    }
    fn find(&mut self, tok: &str) -> Res {
        let k = int(tok)?;
        self.run(|t| {
            t.do_find(k);
            Ok(())
        })
    }
    fn snapshot(&self, msg: &str) -> Step {
        self.snap(msg.to_string(), &[])
    }
    fn clear(&mut self) {
        *self = BinTree::new(self.kind);
    }
    fn random_token(&self, rng: &mut Rng) -> String {
        (rng.range(99) + 1).to_string()
    }
}

// ───────────────────────────── 2b. Heap ─────────────────────────────

/// Binary min-heap stored in an array, drawn as the tree it represents.
struct Heap {
    a: Vec<i64>,
    out: Vec<Step>,
}

impl Heap {
    fn new() -> Self {
        Heap { a: vec![], out: vec![] }
    }

    fn build(&self, i: usize, hot: &[usize]) -> Option<Node> {
        if i >= self.a.len() {
            return None;
        }
        Some(Node {
            label: self.a[i].to_string(),
            tone: Tone::Plain,
            hot: hot.contains(&i),
            kids: vec![self.build(2 * i + 1, hot), self.build(2 * i + 2, hot)],
        })
    }

    fn snap(&self, msg: String, hot: &[usize]) -> Step {
        let items: Vec<String> = self.a.iter().map(|v| v.to_string()).collect();
        let aux = if items.is_empty() { String::new() } else { format!("array: [{}]", items.join(", ")) };
        Step { root: self.build(0, hot), msg, aux }
    }

    fn rec(&mut self, msg: impl Into<String>, hot: &[usize]) {
        let s = self.snap(msg.into(), hot);
        self.out.push(s);
    }

    fn run(&mut self, f: impl FnOnce(&mut Self) -> Result<(), String>) -> Res {
        self.out.clear();
        let r = f(self);
        let out = std::mem::take(&mut self.out);
        r.map(|_| out)
    }

    fn sift_up(&mut self, mut i: usize) {
        while i > 0 {
            let p = (i - 1) / 2;
            let (vi, vp) = (self.a[i], self.a[p]);
            if vi < vp {
                self.rec(format!("{vi} < parent {vp}: swap"), &[i, p]);
                self.a.swap(i, p);
                i = p;
                self.rec("swapped", &[i]);
            } else {
                self.rec(format!("{vi} ≥ parent {vp}: heap property holds"), &[i, p]);
                return;
            }
        }
        self.rec("reached the root", &[0]);
    }

    fn sift_down(&mut self, mut i: usize) {
        loop {
            let (l, r) = (2 * i + 1, 2 * i + 2);
            let mut m = i;
            if l < self.a.len() && self.a[l] < self.a[m] {
                m = l;
            }
            if r < self.a.len() && self.a[r] < self.a[m] {
                m = r;
            }
            if m == i {
                self.rec(format!("{} is not larger than its children: settled", self.a[i]), &[i]);
                return;
            }
            self.rec(format!("{} > child {}: swap with the smaller child", self.a[i], self.a[m]), &[i, m]);
            self.a.swap(i, m);
            i = m;
            self.rec("swapped", &[i]);
        }
    }

    /// Removes position `i`: move the last element there, then restore order.
    fn remove_at(&mut self, i: usize) {
        let last = self.a.len() - 1;
        self.a.swap(i, last);
        let gone = self.a.pop().unwrap_or_default();
        if i < self.a.len() {
            let moved = self.a[i];
            self.rec(format!("removed {gone}; move last element {moved} into its slot"), &[i]);
            let p = i.saturating_sub(1) / 2;
            if i > 0 && self.a[i] < self.a[p] {
                self.sift_up(i);
            } else {
                self.sift_down(i);
            }
        } else {
            self.rec(format!("removed {gone}"), &[]);
        }
    }

    #[cfg(test)]
    fn check(&self) {
        for i in 1..self.a.len() {
            assert!(self.a[(i - 1) / 2] <= self.a[i], "heap property at {i}");
        }
    }
}

impl Tree for Heap {
    fn name(&self) -> &'static str {
        "Heap"
    }
    fn blurb(&self) -> &'static str {
        "min-heap in an array"
    }
    fn lesson(&self) -> &'static [Page] {
        LESSON_HEAP
    }
    fn insert(&mut self, tok: &str) -> Res {
        let k = int(tok)?;
        self.run(|t| {
            t.a.push(k);
            let i = t.a.len() - 1;
            t.rec(format!("append {k} at index {i}"), &[i]);
            t.sift_up(i);
            Ok(())
        })
    }
    fn delete(&mut self, tok: &str) -> Res {
        let k = int(tok)?;
        self.run(|t| {
            let Some(i) = t.a.iter().position(|&v| v == k) else {
                return Err(format!("{k} is not in the heap"));
            };
            t.rec(format!("found {k} at index {i}"), &[i]);
            t.remove_at(i);
            Ok(())
        })
    }
    fn find(&mut self, tok: &str) -> Res {
        let k = int(tok)?;
        self.run(|t| {
            let mut queue = VecDeque::from([0usize]);
            if t.a.is_empty() {
                t.rec("the heap is empty", &[]);
            }
            while let Some(i) = queue.pop_front() {
                if i >= t.a.len() {
                    continue;
                }
                let v = t.a[i];
                if v == k {
                    t.rec(format!("found {k} at index {i}"), &[i]);
                    return Ok(());
                }
                if v > k {
                    t.rec(format!("{v} > {k}: everything below is larger too, prune"), &[i]);
                } else {
                    t.rec(format!("{v} < {k}: look at both children"), &[i]);
                    queue.push_back(2 * i + 1);
                    queue.push_back(2 * i + 2);
                }
            }
            t.rec(format!("{k} is not in the heap"), &[]);
            Ok(())
        })
    }
    fn special(&mut self, cmd: &str, _args: &[&str]) -> Option<Res> {
        if cmd != "pop" && cmd != "extract" {
            return None;
        }
        Some(self.run(|t| {
            if t.a.is_empty() {
                return Err("the heap is empty".into());
            }
            let min = t.a[0];
            t.rec(format!("extract-min: the minimum is the root, {min}"), &[0]);
            t.remove_at(0);
            Ok(())
        }))
    }
    fn snapshot(&self, msg: &str) -> Step {
        self.snap(msg.to_string(), &[])
    }
    fn clear(&mut self) {
        self.a.clear();
    }
    fn random_token(&self, rng: &mut Rng) -> String {
        (rng.range(99) + 1).to_string()
    }
}

// ───────────────────────────── 2c. Trie ─────────────────────────────

struct TN {
    ch: char,
    kids: BTreeMap<char, usize>,
    end: bool,
    parent: usize,
}

struct Trie {
    n: Vec<TN>,
    out: Vec<Step>,
}

const WORDS: &[&str] = &[
    "car", "cart", "cat", "care", "dog", "door", "dot", "tree", "trie", "tea", "ten", "to", "top", "bat", "bar", "bark",
];

impl Trie {
    fn new() -> Self {
        Trie { n: vec![TN { ch: '·', kids: BTreeMap::new(), end: false, parent: 0 }], out: vec![] }
    }

    fn build(&self, i: usize, hot: &[usize]) -> Node {
        let x = &self.n[i];
        let label = if i == 0 { "root".to_string() } else if x.end { format!("{}*", x.ch) } else { x.ch.to_string() };
        Node {
            label,
            tone: if x.end { Tone::End } else { Tone::Plain },
            hot: hot.contains(&i),
            kids: x.kids.values().map(|&c| Some(self.build(c, hot))).collect(),
        }
    }

    fn snap(&self, msg: String, hot: &[usize]) -> Step {
        let root = if self.n[0].kids.is_empty() { None } else { Some(self.build(0, hot)) };
        Step { root, msg, aux: String::new() }
    }

    fn rec(&mut self, msg: impl Into<String>, hot: &[usize]) {
        let s = self.snap(msg.into(), hot);
        self.out.push(s);
    }

    fn run(&mut self, f: impl FnOnce(&mut Self) -> Result<(), String>) -> Res {
        self.out.clear();
        let r = f(self);
        let out = std::mem::take(&mut self.out);
        r.map(|_| out)
    }

    fn word(tok: &str) -> Result<String, String> {
        if tok.is_empty() || !tok.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(format!("'{tok}' is not a word (letters a-z only)"));
        }
        Ok(tok.to_ascii_lowercase())
    }

    fn collect(&self, i: usize, prefix: &mut String, out: &mut Vec<String>) {
        if self.n[i].end {
            out.push(prefix.clone());
        }
        for (&c, &k) in &self.n[i].kids {
            prefix.push(c);
            self.collect(k, prefix, out);
            prefix.pop();
        }
    }

    #[cfg(test)]
    fn words(&self) -> Vec<String> {
        let mut out = vec![];
        self.collect(0, &mut String::new(), &mut out);
        out
    }
}

impl Tree for Trie {
    fn name(&self) -> &'static str {
        "Trie"
    }
    fn blurb(&self) -> &'static str {
        "prefix tree for words"
    }
    fn lesson(&self) -> &'static [Page] {
        LESSON_TRIE
    }
    fn insert(&mut self, tok: &str) -> Res {
        let w = Self::word(tok)?;
        self.run(|t| {
            let mut cur = 0;
            t.rec(format!("insert \"{w}\": start at the root"), &[0]);
            for c in w.chars() {
                if let Some(&next) = t.n[cur].kids.get(&c) {
                    cur = next;
                    t.rec(format!("'{c}' already exists: follow it"), &[cur]);
                } else {
                    t.n.push(TN { ch: c, kids: BTreeMap::new(), end: false, parent: cur });
                    let id = t.n.len() - 1;
                    t.n[cur].kids.insert(c, id);
                    cur = id;
                    t.rec(format!("no '{c}' edge: create a node"), &[cur]);
                }
            }
            if t.n[cur].end {
                return Err(format!("\"{w}\" is already in the trie"));
            }
            t.n[cur].end = true;
            t.rec(format!("mark the end of \"{w}\" (*)"), &[cur]);
            Ok(())
        })
    }
    fn delete(&mut self, tok: &str) -> Res {
        let w = Self::word(tok)?;
        self.run(|t| {
            let mut cur = 0;
            for c in w.chars() {
                match t.n[cur].kids.get(&c) {
                    Some(&next) => cur = next,
                    None => return Err(format!("\"{w}\" is not in the trie")),
                }
            }
            if !t.n[cur].end {
                return Err(format!("\"{w}\" is only a prefix, not a stored word"));
            }
            t.n[cur].end = false;
            t.rec(format!("unmark the end of \"{w}\""), &[cur]);
            while cur != 0 && t.n[cur].kids.is_empty() && !t.n[cur].end {
                let (p, c) = (t.n[cur].parent, t.n[cur].ch);
                t.n[p].kids.remove(&c);
                t.rec(format!("'{c}' leads to no word any more: prune it"), &[p]);
                cur = p;
            }
            Ok(())
        })
    }
    fn find(&mut self, tok: &str) -> Res {
        let w = Self::word(tok)?;
        self.run(|t| {
            let mut cur = 0;
            t.rec(format!("search \"{w}\": start at the root"), &[0]);
            for c in w.chars() {
                match t.n[cur].kids.get(&c).copied() {
                    Some(next) => {
                        cur = next;
                        t.rec(format!("follow '{c}'"), &[cur]);
                    }
                    None => {
                        t.rec(format!("no '{c}' edge: \"{w}\" is not in the trie"), &[cur]);
                        return Ok(());
                    }
                }
            }
            if t.n[cur].end {
                t.rec(format!("found \"{w}\" (the node is marked *)"), &[cur]);
            } else {
                t.rec(format!("\"{w}\" is a prefix of other words but not a word itself"), &[cur]);
            }
            Ok(())
        })
    }
    fn special(&mut self, cmd: &str, args: &[&str]) -> Option<Res> {
        if cmd != "complete" {
            return None;
        }
        let prefix = args.first().copied().unwrap_or("");
        Some(Self::word(prefix).and_then(|p| {
            self.run(|t| {
                let mut cur = 0;
                for c in p.chars() {
                    match t.n[cur].kids.get(&c).copied() {
                        Some(next) => cur = next,
                        None => {
                            t.rec(format!("nothing starts with \"{p}\""), &[]);
                            return Ok(());
                        }
                    }
                }
                let mut found = vec![];
                t.collect(cur, &mut p.clone(), &mut found);
                t.rec(format!("words starting with \"{p}\": {}", found.join(", ")), &[cur]);
                Ok(())
            })
        }))
    }
    fn snapshot(&self, msg: &str) -> Step {
        self.snap(msg.to_string(), &[])
    }
    fn clear(&mut self) {
        *self = Trie::new();
    }
    fn random_token(&self, rng: &mut Rng) -> String {
        WORDS[rng.range(WORDS.len() as u64) as usize].to_string()
    }
}

// ──────────────────────────── 2d. B-Tree ────────────────────────────

/// Minimum degree. t = 2 gives the classic 2-3-4 tree (1..=3 keys per node).
const T: usize = 2;

struct BN {
    keys: Vec<i64>,
    kids: Vec<usize>,
    leaf: bool,
}

struct BTree {
    n: Vec<BN>,
    root: usize,
    out: Vec<Step>,
}

impl BTree {
    fn new() -> Self {
        BTree { n: vec![BN { keys: vec![], kids: vec![], leaf: true }], root: 0, out: vec![] }
    }

    fn lab(&self, x: usize) -> String {
        self.n[x].keys.iter().map(|k| k.to_string()).collect::<Vec<_>>().join(" ")
    }

    fn build(&self, x: usize, hot: &[usize]) -> Node {
        Node {
            label: self.lab(x),
            tone: Tone::Plain,
            hot: hot.contains(&x),
            kids: self.n[x].kids.iter().map(|&c| Some(self.build(c, hot))).collect(),
        }
    }

    fn snap(&self, msg: String, hot: &[usize]) -> Step {
        let empty = self.n[self.root].keys.is_empty();
        Step { root: if empty { None } else { Some(self.build(self.root, hot)) }, msg, aux: String::new() }
    }

    fn rec(&mut self, msg: impl Into<String>, hot: &[usize]) {
        let s = self.snap(msg.into(), hot);
        self.out.push(s);
    }

    fn run(&mut self, f: impl FnOnce(&mut Self) -> Result<(), String>) -> Res {
        self.out.clear();
        let r = f(self);
        let out = std::mem::take(&mut self.out);
        r.map(|_| out)
    }

    fn pos(&self, x: usize, k: i64) -> usize {
        self.n[x].keys.iter().position(|&q| q >= k).unwrap_or(self.n[x].keys.len())
    }

    fn contains(&self, k: i64) -> bool {
        let mut x = self.root;
        loop {
            let i = self.pos(x, k);
            if i < self.n[x].keys.len() && self.n[x].keys[i] == k {
                return true;
            }
            if self.n[x].leaf {
                return false;
            }
            x = self.n[x].kids[i];
        }
    }

    fn alloc(&mut self, leaf: bool) -> usize {
        self.n.push(BN { keys: vec![], kids: vec![], leaf });
        self.n.len() - 1
    }

    fn split_child(&mut self, x: usize, i: usize) {
        let y = self.n[x].kids[i];
        let z = self.alloc(self.n[y].leaf);
        let right_keys = self.n[y].keys.split_off(T);
        let mid = self.n[y].keys.pop().unwrap_or_default();
        self.n[z].keys = right_keys;
        if !self.n[y].leaf {
            let right_kids = self.n[y].kids.split_off(T);
            self.n[z].kids = right_kids;
        }
        self.n[x].keys.insert(i, mid);
        self.n[x].kids.insert(i + 1, z);
    }

    fn do_insert(&mut self, k: i64) -> Result<(), String> {
        if self.contains(k) {
            return Err(format!("{k} is already in the tree"));
        }
        if self.n[self.root].keys.len() == 2 * T - 1 {
            let old = self.root;
            self.rec(format!("root [{}] is full: split it, the tree grows taller", self.lab(old)), &[old]);
            let s = self.alloc(false);
            self.n[s].kids.push(old);
            self.root = s;
            self.split_child(s, 0);
            self.rec(format!("median {} moved up into a new root", self.n[s].keys[0]), &[s]);
        }
        let mut x = self.root;
        loop {
            if self.n[x].leaf {
                let i = self.pos(x, k);
                self.n[x].keys.insert(i, k);
                self.rec(format!("insert {k} into the leaf"), &[x]);
                return Ok(());
            }
            let mut i = self.pos(x, k);
            let c = self.n[x].kids[i];
            self.rec(format!("at [{}]: {k} belongs in child {}", self.lab(x), i), &[x]);
            if self.n[c].keys.len() == 2 * T - 1 {
                self.rec(format!("child [{}] is full: split it before going down", self.lab(c)), &[c]);
                self.split_child(x, i);
                self.rec(format!("median {} moved up into [{}]", self.n[x].keys[i], self.lab(x)), &[x]);
                if k > self.n[x].keys[i] {
                    i += 1;
                }
            }
            x = self.n[x].kids[i];
        }
    }

    fn extreme(&self, mut x: usize, max: bool) -> i64 {
        while !self.n[x].leaf {
            x = if max { *self.n[x].kids.last().unwrap_or(&x) } else { self.n[x].kids[0] };
        }
        if max { *self.n[x].keys.last().unwrap_or(&0) } else { self.n[x].keys[0] }
    }

    fn merge(&mut self, x: usize, i: usize) {
        let y = self.n[x].kids[i];
        let z = self.n[x].kids.remove(i + 1);
        let m = self.n[x].keys.remove(i);
        let zk = std::mem::take(&mut self.n[z].keys);
        let zc = std::mem::take(&mut self.n[z].kids);
        self.n[y].keys.push(m);
        self.n[y].keys.extend(zk);
        self.n[y].kids.extend(zc);
        if x == self.root && self.n[x].keys.is_empty() {
            self.root = y;
        }
    }

    fn do_delete(&mut self, mut k: i64) -> Result<(), String> {
        if !self.contains(k) {
            return Err(format!("{k} is not in the tree"));
        }
        let mut x = self.root;
        loop {
            let i = self.pos(x, k);
            let found = i < self.n[x].keys.len() && self.n[x].keys[i] == k;
            if found {
                if self.n[x].leaf {
                    self.n[x].keys.remove(i);
                    self.rec(format!("removed {k} from the leaf"), &[x]);
                    return Ok(());
                }
                let (y, z) = (self.n[x].kids[i], self.n[x].kids[i + 1]);
                if self.n[y].keys.len() >= T {
                    let pred = self.extreme(y, true);
                    self.rec(format!("{k} is in an internal node: replace it with its predecessor {pred}"), &[x, y]);
                    self.n[x].keys[i] = pred;
                    k = pred;
                    x = y;
                } else if self.n[z].keys.len() >= T {
                    let succ = self.extreme(z, false);
                    self.rec(format!("{k} is in an internal node: replace it with its successor {succ}"), &[x, z]);
                    self.n[x].keys[i] = succ;
                    k = succ;
                    x = z;
                } else {
                    self.rec(format!("both children of {k} are minimal: merge them with {k}"), &[y, z]);
                    self.merge(x, i);
                    self.rec(format!("merged into [{}]", self.lab(y)), &[y]);
                    x = y;
                }
                continue;
            }
            let mut i = i;
            let c = self.n[x].kids[i];
            self.rec(format!("at [{}]: {k} is in child {i}", self.lab(x)), &[x]);
            if self.n[c].keys.len() < T {
                let has_left = i > 0 && self.n[self.n[x].kids[i - 1]].keys.len() >= T;
                let has_right = i < self.n[x].keys.len() && self.n[self.n[x].kids[i + 1]].keys.len() >= T;
                if has_left {
                    let l = self.n[x].kids[i - 1];
                    self.rec(format!("child [{}] is minimal: borrow through the parent from the left sibling", self.lab(c)), &[c, l, x]);
                    let down = self.n[x].keys[i - 1];
                    let up = self.n[l].keys.pop().unwrap_or_default();
                    self.n[c].keys.insert(0, down);
                    self.n[x].keys[i - 1] = up;
                    if !self.n[l].leaf {
                        let moved = self.n[l].kids.pop().unwrap_or_default();
                        self.n[c].kids.insert(0, moved);
                    }
                    self.rec("borrowed a key (a rotation through the parent)", &[c]);
                } else if has_right {
                    let r = self.n[x].kids[i + 1];
                    self.rec(format!("child [{}] is minimal: borrow through the parent from the right sibling", self.lab(c)), &[c, r, x]);
                    let down = self.n[x].keys[i];
                    let up = self.n[r].keys.remove(0);
                    self.n[c].keys.push(down);
                    self.n[x].keys[i] = up;
                    if !self.n[r].leaf {
                        let moved = self.n[r].kids.remove(0);
                        self.n[c].kids.push(moved);
                    }
                    self.rec("borrowed a key (a rotation through the parent)", &[c]);
                } else {
                    if i == self.n[x].keys.len() {
                        i -= 1;
                    }
                    let (a, b) = (self.n[x].kids[i], self.n[x].kids[i + 1]);
                    self.rec(format!("child [{}] and its sibling are minimal: merge them with the key between", self.lab(c)), &[a, b]);
                    self.merge(x, i);
                    self.rec(format!("merged into [{}]", self.lab(a)), &[a]);
                    if x == self.root && self.n[x].keys.is_empty() {
                        // merge() already moved the root down; x is now garbage.
                    }
                    x = a;
                    continue;
                }
            }
            x = self.n[x].kids[i];
        }
    }

    fn do_find(&mut self, k: i64) {
        let mut x = self.root;
        if self.n[x].keys.is_empty() {
            self.rec("the tree is empty", &[]);
            return;
        }
        loop {
            let i = self.pos(x, k);
            if i < self.n[x].keys.len() && self.n[x].keys[i] == k {
                self.rec(format!("found {k} in [{}]", self.lab(x)), &[x]);
                return;
            }
            if self.n[x].leaf {
                self.rec(format!("reached a leaf without {k}: not in the tree"), &[x]);
                return;
            }
            self.rec(format!("at [{}]: {k} is not here, go to child {i}", self.lab(x)), &[x]);
            x = self.n[x].kids[i];
        }
    }

    #[cfg(test)]
    fn check(&self) -> Vec<i64> {
        fn go(t: &BTree, x: usize, depth: usize, leaf_depth: &mut Option<usize>, out: &mut Vec<i64>) {
            let b = &t.n[x];
            if x != t.root {
                assert!(b.keys.len() >= T - 1 && b.keys.len() <= 2 * T - 1, "key count {}", b.keys.len());
            }
            assert!(b.keys.windows(2).all(|w| w[0] < w[1]), "keys unsorted");
            if b.leaf {
                assert!(b.kids.is_empty());
                match leaf_depth {
                    Some(d) => assert_eq!(*d, depth, "leaves at different depths"),
                    None => *leaf_depth = Some(depth),
                }
                out.extend(&b.keys);
            } else {
                assert_eq!(b.kids.len(), b.keys.len() + 1);
                for (i, &c) in b.kids.iter().enumerate() {
                    go(t, c, depth + 1, leaf_depth, out);
                    if i < b.keys.len() {
                        out.push(b.keys[i]);
                    }
                }
            }
        }
        let mut out = vec![];
        go(self, self.root, 0, &mut None, &mut out);
        assert!(out.windows(2).all(|w| w[0] < w[1]), "in-order traversal is not sorted");
        out
    }
}

impl Tree for BTree {
    fn name(&self) -> &'static str {
        "B-Tree"
    }
    fn blurb(&self) -> &'static str {
        "wide nodes, t = 2"
    }
    fn lesson(&self) -> &'static [Page] {
        LESSON_BTREE
    }
    fn insert(&mut self, tok: &str) -> Res {
        let k = int(tok)?;
        self.run(|t| t.do_insert(k))
    }
    fn delete(&mut self, tok: &str) -> Res {
        let k = int(tok)?;
        self.run(|t| t.do_delete(k))
    }
    fn find(&mut self, tok: &str) -> Res {
        let k = int(tok)?;
        self.run(|t| {
            t.do_find(k);
            Ok(())
        })
    }
    fn snapshot(&self, msg: &str) -> Step {
        self.snap(msg.to_string(), &[])
    }
    fn clear(&mut self) {
        *self = BTree::new();
    }
    fn random_token(&self, rng: &mut Rng) -> String {
        (rng.range(99) + 1).to_string()
    }
}

fn make_tree(name: &str) -> Option<Box<dyn Tree>> {
    Some(match name.to_ascii_lowercase().as_str() {
        "bst" => Box::new(BinTree::new(Kind::Bst)),
        "avl" => Box::new(BinTree::new(Kind::Avl)),
        "rb" | "rbt" | "red-black" | "redblack" => Box::new(BinTree::new(Kind::Rb)),
        "btree" | "b-tree" | "b" => Box::new(BTree::new()),
        "trie" => Box::new(Trie::new()),
        "heap" => Box::new(Heap::new()),
        _ => return None,
    })
}

const TREE_NAMES: [&str; 6] = ["bst", "avl", "rb", "btree", "trie", "heap"];

// ───────────────────────────── 3. layout ─────────────────────────────

type Cell = (char, Style);

struct Rendered {
    grid: Vec<Vec<Cell>>,
    /// x, y, width of the highlighted node, if any (for auto-follow).
    hot: Option<(usize, usize, usize)>,
}

struct Item {
    x: i32,
    y: i32,
    w: i32,
    text: String,
    tone: Tone,
    hot: bool,
    kids: Vec<i32>,
}

const EDGE: Color = Color::Rgb(110, 118, 150);

fn node_style(tone: Tone, hot: bool) -> Style {
    if hot {
        return Style::new().fg(Color::Rgb(20, 20, 20)).bg(Color::Rgb(250, 204, 21)).add_modifier(Modifier::BOLD);
    }
    let (fg, bg) = match tone {
        Tone::Plain => (Color::Rgb(205, 214, 255), Color::Rgb(44, 50, 84)),
        Tone::Red => (Color::White, Color::Rgb(190, 42, 62)),
        Tone::Black => (Color::White, Color::Rgb(58, 58, 66)),
        Tone::End => (Color::White, Color::Rgb(26, 112, 78)),
    };
    Style::new().fg(fg).bg(bg).add_modifier(Modifier::BOLD)
}

fn node_text(n: &Node, plain: bool) -> String {
    match (plain, n.tone) {
        (true, Tone::Red) => format!("{}(R)", n.label),
        (true, Tone::Black) => format!("{}(B)", n.label),
        _ => n.label.clone(),
    }
}

/// Places the subtree starting at column `*cur`; returns the node's center.
fn place(n: &Node, depth: i32, cur: &mut i32, out: &mut Vec<Item>, plain: bool) -> i32 {
    let text = node_text(n, plain);
    let w = text.chars().count() as i32 + 2;
    let start = *cur;
    let base = out.len();
    let mut centers = vec![];
    let mut slots = vec![];
    for k in &n.kids {
        match k {
            Some(c) => {
                let cc = place(c, depth + 1, cur, out, plain);
                centers.push(cc);
                slots.push(cc);
            }
            None if n.kids.iter().any(|k| k.is_some()) => {
                slots.push(*cur + 1);
                *cur += 3;
            }
            None => {}
        }
    }
    let mut center = if slots.is_empty() { start + w / 2 } else { (slots[0] + slots[slots.len() - 1]) / 2 };
    let left = center - w / 2;
    if left < start {
        let d = start - left;
        for it in &mut out[base..] {
            it.x += d;
            it.kids.iter_mut().for_each(|c| *c += d);
        }
        centers.iter_mut().for_each(|c| *c += d);
        center += d;
        *cur += d;
    }
    *cur = (*cur).max(center - w / 2 + w + 1);
    out.push(Item { x: center - w / 2, y: depth * 2, w, text, tone: n.tone, hot: n.hot, kids: centers });
    center
}

fn junction(u: bool, d: bool, l: bool, r: bool) -> char {
    match (u, d, l, r) {
        (false, false, true, true) => '─',
        (false, true, false, true) => '┌',
        (false, true, true, false) => '┐',
        (false, true, true, true) => '┬',
        (true, false, true, true) => '┴',
        (true, false, false, true) => '└',
        (true, false, true, false) => '┘',
        (true, true, true, true) => '┼',
        (true, true, false, true) => '├',
        (true, true, true, false) => '┤',
        (true, true, false, false) | (false, true, false, false) | (true, false, false, false) => '│',
        _ => ' ',
    }
}

fn render(root: &Node, plain: bool) -> Rendered {
    let mut items = vec![];
    let mut cur = 0;
    place(root, 0, &mut cur, &mut items, plain);
    let minx = items.iter().map(|i| i.x).min().unwrap_or(0);
    let maxx = items.iter().map(|i| i.x + i.w).max().unwrap_or(0);
    let maxy = items.iter().map(|i| i.y).max().unwrap_or(0);
    let (width, height) = ((maxx - minx) as usize, maxy as usize + 1);
    let mut grid = vec![vec![(' ', Style::default()); width]; height];
    let edge = Style::new().fg(EDGE);
    for it in items.iter().filter(|i| !i.kids.is_empty()) {
        let cx = it.x + it.w / 2;
        let lo = cx.min(*it.kids.iter().min().unwrap_or(&cx));
        let hi = cx.max(*it.kids.iter().max().unwrap_or(&cx));
        for x in lo..=hi {
            let ch = junction(x == cx, it.kids.contains(&x), x > lo, x < hi);
            grid[it.y as usize + 1][(x - minx) as usize] = (ch, edge);
        }
    }
    let mut hot = None;
    for it in &items {
        let style = node_style(it.tone, it.hot);
        let text = format!(" {} ", it.text);
        for (j, ch) in text.chars().enumerate() {
            grid[it.y as usize][(it.x - minx) as usize + j] = (ch, style);
        }
        if it.hot {
            hot = Some(((it.x - minx) as usize, it.y as usize, it.w as usize));
        }
    }
    Rendered { grid, hot }
}

fn grid_line<'a>(cells: &[Cell]) -> Vec<Span<'a>> {
    let mut spans = vec![];
    let mut buf = String::new();
    let mut style = Style::default();
    for (i, &(ch, st)) in cells.iter().enumerate() {
        if i > 0 && st != style {
            spans.push(Span::styled(std::mem::take(&mut buf), style));
        }
        style = st;
        buf.push(ch);
    }
    if !buf.is_empty() {
        spans.push(Span::styled(buf, style));
    }
    spans
}

// ───────────────────────────── 4. lessons ─────────────────────────────

const LESSON_BST: &[Page] = &[
    ("Concept", "## Binary Search Tree
Each node holds one key and at most two children.
• left subtree  → keys smaller than the node
• right subtree → keys larger than the node
Searching is a guided walk: compare, go left or right, repeat. Every comparison throws away a whole subtree."),
    ("Operations", "## Operations
• find    walk down comparing at each node
• insert  walk down like a find, attach the key as a new leaf
• delete  leaf: drop it · one child: lift the child · two children: copy the in-order successor (smallest key of the right subtree) into its place
An in-order walk (left, node, right) yields the keys sorted."),
    ("Complexity", "## Complexity and pitfalls
Cost is O(height). Random input gives height ≈ 2·log n, but sorted input degenerates into a linked list: O(n).
Try it:  :insert 1 2 3 4 5 6 7  — then the same on the AVL tab.
Use it when: keys arrive in random order and you want a simple ordered map. Otherwise prefer a self-balancing tree."),
];

const LESSON_AVL: &[Page] = &[
    ("Concept", "## AVL tree
A BST that keeps itself balanced. For every node:
    balance = height(left) − height(right) ∈ {−1, 0, +1}
The number next to each key is its balance. The moment a node reaches ±2, a rotation repairs it, so height stays O(log n)."),
    ("Rotations", "## The four cases
• left-left    → rotate right
• right-right  → rotate left
• left-right   → rotate left at the child, then right
• right-left   → mirror image
A rotation keeps the in-order sequence intact; it only changes who is the parent.
Try:  :insert 1 2 3   :insert 3 2 1   :insert 3 1 2   :insert 1 3 2"),
    ("Complexity", "## Complexity and trade-offs
Height ≤ 1.44·log2(n). find/insert/delete are O(log n).
Insert needs at most one rotation (single or double); delete may rotate at every level on the way up.
Stricter than red-black: faster lookups, more rotation work on writes — a good fit for read-heavy data."),
];

const LESSON_RB: &[Page] = &[
    ("Rules", "## Red-Black tree
Every node is red or black (black shown gray).
1. the root is black
2. a red node never has a red child
3. every path from a node to an empty slot has the same number of black nodes
Together they guarantee the longest path is at most twice the shortest."),
    ("Insert", "## Insert
The new node is red. If its parent is black, done. Otherwise look at the uncle:
• uncle red   → recolor parent and uncle black, grandparent red, continue upward
• uncle black → one or two rotations plus a recolor, then stop
Finally the root is repainted black.
Try:  :insert 1 2 3 4 5 6 7 8"),
    ("Delete", "## Delete
Removing a black node leaves one path short of black (double black). The fix looks at the sibling:
• sibling red                → rotate it into a black case
• sibling black, kids black  → recolor sibling red, push the problem up
• sibling has a red child    → rotate and recolor, done
At most 3 rotations per delete."),
    ("Complexity", "## Complexity and use
O(log n) for everything, height ≤ 2·log2(n+1). Fewer rotations than AVL on writes, slightly taller trees.
Found in C++ std::map, Java TreeMap and the Linux scheduler."),
];

const LESSON_BTREE: &[Page] = &[
    ("Concept", "## B-Tree (minimum degree t = 2)
A balanced search tree with wide nodes, built for disks and databases.
• every node holds 1–3 keys (the root may hold fewer)
• an internal node with k keys has k+1 children
• all leaves sit at the same depth
Here t = 2, which makes it a 2-3-4 tree."),
    ("Insert", "## Insert
Insertion always ends in a leaf. Full nodes (3 keys) are split on the way down: the median key moves up and the node becomes two.
The tree only grows taller when the root itself splits, so it grows from the top and stays perfectly balanced.
Try:  :insert 10 20 30 40 50 60 70"),
    ("Delete", "## Delete
Before descending into a child with only one key, repair it:
• borrow a key from a sibling through the parent, or
• merge with a sibling and pull the separator down
A key in an internal node is replaced by its predecessor or successor. If the root loses its last key, the tree shrinks by one level."),
    ("Why", "## Why wide nodes?
One node = one disk page. Fewer levels means fewer reads: with ~1000 keys per node, 3 levels index a billion rows.
Databases and file systems (PostgreSQL, SQLite, NTFS) use B-trees and their B+tree variant."),
];

const LESSON_TRIE: &[Page] = &[
    ("Concept", "## Trie (prefix tree)
Edges are letters; the path from the root spells a prefix. A node marked * ends a stored word.
Words with a common prefix share nodes: car, cart, care and cat all start with the path c → a.
Only lowercase letters here:  :insert car cart cat"),
    ("Operations", "## Operations
• insert / find cost O(L), L = word length — independent of how many words are stored
• delete: unmark the end, then prune nodes that no longer lead to any word
• :complete ca  lists every word starting with a prefix
A node being found but not marked * means the word is only a prefix of other words."),
    ("Trade-offs", "## Trade-offs
One node per letter, each with a child map: memory hungry. Radix trees merge single-child chains to save space.
Use it for autocomplete, spell checking, IP routing (longest prefix match) and word games."),
];

const LESSON_HEAP: &[Page] = &[
    ("Concept", "## Binary min-heap
A complete binary tree stored in a plain array. Rule: every parent ≤ its children, so the minimum is always the root.
For index i:  parent = (i−1)/2,  children = 2i+1 and 2i+2. No pointers needed — the array shown under the tree is the real storage."),
    ("Operations", "## Operations
• insert   append at the end, sift up while smaller than the parent
• :pop     remove the root: move the last element there, sift down by swapping with the smaller child
Both O(log n); peek (read the root) is O(1). Building a heap from n items bottom-up is O(n)."),
    ("Trade-offs", "## Trade-offs
A heap is not sorted: find is O(n) (we prune subtrees whose root exceeds the target).
Use it for priority queues, Dijkstra, heap sort, top-k and schedulers. If you need ordered iteration or cheap arbitrary delete, use a balanced BST."),
];

// ───────────────────────────── 5. app (TUI) ─────────────────────────────

const ACCENT: Color = Color::Rgb(130, 170, 255);
const DIM: Color = Color::Rgb(120, 126, 150);
const GOOD: Color = Color::Rgb(120, 220, 160);
const BAD: Color = Color::Rgb(255, 120, 130);

struct Session {
    tree: Box<dyn Tree>,
    steps: Vec<Step>,
    cur: usize,
}

impl Session {
    fn new(tree: Box<dyn Tree>) -> Self {
        let hint = match tree.name() {
            "Trie" => "empty trie — press i and type some words, e.g. car cart cat",
            _ => "empty tree — press i and type some numbers, e.g. 5 3 8 1 4",
        };
        let first = tree.snapshot(hint);
        Session { tree, steps: vec![first], cur: 0 }
    }
}

#[derive(PartialEq)]
enum Mode {
    Normal,
    Command,
}

struct App {
    sessions: Vec<Session>,
    sel: usize,
    mode: Mode,
    input: String,
    history: Vec<String>,
    hist_pos: Option<usize>,
    status: Option<(String, bool)>,
    playing: bool,
    autoplay: bool,
    speed_ms: u64,
    last_tick: Instant,
    ox: usize,
    oy: usize,
    follow: bool,
    page: usize,
    lscroll: u16,
    help: bool,
    rng: Rng,
    quit: bool,
}

impl App {
    fn new() -> Self {
        App {
            sessions: TREE_NAMES.iter().filter_map(|n| make_tree(n)).map(Session::new).collect(),
            sel: 0,
            mode: Mode::Normal,
            input: String::new(),
            history: vec![],
            hist_pos: None,
            status: None,
            playing: false,
            autoplay: true,
            speed_ms: 700,
            last_tick: Instant::now(),
            ox: 0,
            oy: 0,
            follow: true,
            page: 0,
            lscroll: 0,
            help: false,
            rng: Rng::new(),
            quit: false,
        }
    }

    fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.quit {
            terminal.draw(|f| self.draw(f))?;
            if event::poll(Duration::from_millis(40))? {
                if let Event::Key(k) = event::read()? {
                    if k.kind == KeyEventKind::Press {
                        self.on_key(k);
                    }
                }
            }
            self.tick();
        }
        Ok(())
    }

    fn tick(&mut self) {
        if self.playing && self.last_tick.elapsed() >= Duration::from_millis(self.speed_ms) {
            self.last_tick = Instant::now();
            self.step(1);
        }
    }

    fn ok(&mut self, m: impl Into<String>) {
        self.status = Some((m.into(), false));
    }
    fn err(&mut self, m: impl Into<String>) {
        self.status = Some((m.into(), true));
    }

    fn select(&mut self, i: usize) {
        self.sel = i % self.sessions.len();
        self.playing = false;
        self.page = 0;
        self.lscroll = 0;
        self.ox = 0;
        self.oy = 0;
        self.follow = true;
    }

    fn step(&mut self, d: i32) {
        let s = &mut self.sessions[self.sel];
        let next = (s.cur as i32 + d).clamp(0, s.steps.len() as i32 - 1) as usize;
        if next == s.cur || (d > 0 && next + 1 == s.steps.len()) {
            self.playing = false;
        }
        s.cur = next;
        self.follow = true;
    }

    fn goto(&mut self, i: usize) {
        let s = &mut self.sessions[self.sel];
        s.cur = i.min(s.steps.len() - 1);
        self.follow = true;
        self.playing = false;
    }

    // ---- commands ----

    fn exec(&mut self, line: &str) {
        let words: Vec<&str> = line.split_whitespace().collect();
        let Some((&cmd, args)) = words.split_first() else { return };
        match cmd {
            "insert" | "i" | "add" => self.op(args, |t, a| t.insert(a)),
            "delete" | "d" | "del" | "rm" => self.op(args, |t, a| t.delete(a)),
            "find" | "f" | "search" | "s" => self.op(args, |t, a| t.find(a)),
            "clear" | "c" => {
                let s = &mut self.sessions[self.sel];
                s.tree.clear();
                *s = Session::new(make_tree(&TREE_NAMES[self.sel]).expect("known tree"));
                self.playing = false;
                self.ok("cleared");
            }
            "random" | "rand" | "r" => {
                let n = args.first().and_then(|a| a.parse::<usize>().ok()).unwrap_or(1).clamp(1, 50);
                let mut toks = vec![];
                for _ in 0..n {
                    toks.push(self.sessions[self.sel].tree.random_token(&mut self.rng));
                }
                let refs: Vec<&str> = toks.iter().map(String::as_str).collect();
                self.op_quiet(&refs);
            }
            "tree" | "t" | "use" => match args.first().and_then(|a| TREE_NAMES.iter().position(|n| make_tree(a).is_some_and(|t| make_tree(n).is_some_and(|u| u.name() == t.name())))) {
                Some(i) => self.select(i),
                None => self.err(format!("usage: tree <{}>", TREE_NAMES.join("|"))),
            },
            "speed" => match args.first().and_then(|a| a.parse::<u64>().ok()) {
                Some(ms) => {
                    self.speed_ms = ms.clamp(50, 3000);
                    self.ok(format!("step delay {} ms", self.speed_ms));
                }
                None => self.err("usage: speed <milliseconds>"),
            },
            "autoplay" | "anim" => {
                self.autoplay = match args.first().copied() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => !self.autoplay,
                };
                self.ok(format!("autoplay {}", if self.autoplay { "on" } else { "off" }));
            }
            "help" | "?" => self.help = true,
            "quit" | "q" | "exit" => self.quit = true,
            other => {
                let res = self.sessions[self.sel].tree.special(other, args);
                match res {
                    Some(Ok(steps)) => self.push_steps(steps),
                    Some(Err(e)) => self.err(e),
                    None => self.err(format!("unknown command '{other}' — press ? for help")),
                }
            }
        }
    }

    fn op(&mut self, toks: &[&str], f: impl Fn(&mut dyn Tree, &str) -> Res) {
        if toks.is_empty() {
            self.err("missing argument — e.g. insert 5 3 8");
            return;
        }
        let mut all = vec![];
        let mut errs = vec![];
        for t in toks {
            match f(self.sessions[self.sel].tree.as_mut(), t) {
                Ok(v) => all.extend(v),
                Err(e) => errs.push(e),
            }
        }
        self.push_steps(all);
        if !errs.is_empty() {
            self.err(errs.join("; "));
        }
    }

    /// Like `op(insert)` but silently skips duplicates (used by `random`).
    fn op_quiet(&mut self, toks: &[&str]) {
        let mut all = vec![];
        for t in toks {
            if let Ok(v) = self.sessions[self.sel].tree.insert(t) {
                all.extend(v);
            }
        }
        self.push_steps(all);
    }

    fn push_steps(&mut self, steps: Vec<Step>) {
        if steps.is_empty() {
            return;
        }
        let s = &mut self.sessions[self.sel];
        if s.steps.len() > 1500 {
            let keep = s.steps.len() - 1;
            s.steps.drain(..keep);
            s.cur = 0;
        }
        let first = s.steps.len();
        s.steps.extend(steps);
        let n = s.steps.len() - first;
        if self.autoplay && n > 1 {
            s.cur = first;
            self.playing = true;
        } else {
            s.cur = s.steps.len() - 1;
            self.playing = false;
        }
        self.last_tick = Instant::now();
        self.follow = true;
        self.status = None;
    }

    // ---- input ----

    fn on_key(&mut self, k: KeyEvent) {
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if self.help {
            self.help = false;
            return;
        }
        match self.mode {
            Mode::Command => self.on_key_command(k),
            Mode::Normal => self.on_key_normal(k),
        }
    }

    fn on_key_command(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.input.clear();
            }
            KeyCode::Enter => {
                let line = std::mem::take(&mut self.input);
                self.mode = Mode::Normal;
                self.hist_pos = None;
                if !line.trim().is_empty() {
                    if self.history.last() != Some(&line) {
                        self.history.push(line.clone());
                    }
                    self.exec(&line);
                }
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Up => {
                if !self.history.is_empty() {
                    let p = self.hist_pos.map_or(self.history.len() - 1, |p| p.saturating_sub(1));
                    self.hist_pos = Some(p);
                    self.input = self.history[p].clone();
                }
            }
            KeyCode::Down => {
                if let Some(p) = self.hist_pos {
                    if p + 1 < self.history.len() {
                        self.hist_pos = Some(p + 1);
                        self.input = self.history[p + 1].clone();
                    } else {
                        self.hist_pos = None;
                        self.input.clear();
                    }
                }
            }
            KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => self.input.clear(),
            KeyCode::Char(c) => self.input.push(c),
            _ => {}
        }
    }

    fn start_command(&mut self, prefill: &str) {
        self.mode = Mode::Command;
        self.input = prefill.to_string();
        self.hist_pos = None;
        self.status = None;
    }

    fn on_key_normal(&mut self, k: KeyEvent) {
        self.status = None;
        let pan = 4;
        match k.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char(':') => self.start_command(""),
            KeyCode::Char('i') => self.start_command("insert "),
            KeyCode::Char('d') => self.start_command("delete "),
            KeyCode::Char('f') => self.start_command("find "),
            KeyCode::Char('r') => self.exec("random"),
            KeyCode::Char('c') => self.exec("clear"),
            KeyCode::Char('a') => self.exec("autoplay"),
            KeyCode::Char('?') => self.help = true,
            KeyCode::Tab | KeyCode::Down => self.select(self.sel + 1),
            KeyCode::BackTab | KeyCode::Up => self.select(self.sel + self.sessions.len() - 1),
            KeyCode::Char(c @ '1'..='9') => {
                let i = c as usize - '1' as usize;
                if i < self.sessions.len() {
                    self.select(i);
                }
            }
            KeyCode::Right => self.step(1),
            KeyCode::Left => self.step(-1),
            KeyCode::Home => self.goto(0),
            KeyCode::End => self.goto(usize::MAX),
            KeyCode::Char(' ') => {
                let s = &mut self.sessions[self.sel];
                if s.cur + 1 >= s.steps.len() {
                    s.cur = 0;
                }
                self.playing = !self.playing;
                self.last_tick = Instant::now();
                self.follow = true;
            }
            KeyCode::Char('+') | KeyCode::Char('=') => self.speed_ms = self.speed_ms.saturating_sub(100).max(50),
            KeyCode::Char('-') => self.speed_ms = (self.speed_ms + 100).min(3000),
            KeyCode::Char('[') => {
                self.page = self.page.saturating_sub(1);
                self.lscroll = 0;
            }
            KeyCode::Char(']') => {
                let n = self.sessions[self.sel].tree.lesson().len();
                self.page = (self.page + 1).min(n - 1);
                self.lscroll = 0;
            }
            KeyCode::PageDown => self.lscroll += 3,
            KeyCode::PageUp => self.lscroll = self.lscroll.saturating_sub(3),
            KeyCode::Char('h') => {
                self.ox = self.ox.saturating_sub(pan);
                self.follow = false;
            }
            KeyCode::Char('l') => {
                self.ox += pan;
                self.follow = false;
            }
            KeyCode::Char('k') => {
                self.oy = self.oy.saturating_sub(2);
                self.follow = false;
            }
            KeyCode::Char('j') => {
                self.oy += 2;
                self.follow = false;
            }
            _ => {}
        }
    }

    // ---- drawing ----

    fn draw(&mut self, f: &mut Frame) {
        let area = f.area();
        let rows = Layout::vertical([Constraint::Min(6), Constraint::Length(1), Constraint::Length(1)]).split(area);
        let side = if area.width >= 100 { 24 } else if area.width >= 70 { 16 } else { 11 };
        let cols = Layout::horizontal([Constraint::Length(side), Constraint::Min(20)]).split(rows[0]);
        self.draw_sidebar(f, cols[0], side >= 24);
        let bottom = (cols[1].height * 2 / 5).clamp(7, 14);
        let right = Layout::vertical([Constraint::Min(4), Constraint::Length(bottom)]).split(cols[1]);
        self.draw_canvas(f, right[0]);
        if cols[1].width >= 70 {
            let b = Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)]).split(right[1]);
            self.draw_lesson(f, b[0]);
            self.draw_events(f, b[1]);
        } else {
            self.draw_lesson(f, right[1]);
        }
        self.draw_command(f, rows[1]);
        self.draw_hints(f, rows[2]);
        if self.help {
            self.draw_help(f, area);
        }
    }

    fn block(title: &str, active: bool) -> Block<'static> {
        Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(if active { ACCENT } else { Color::Rgb(70, 76, 104) }))
            .title(Span::styled(format!(" {title} "), Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)))
    }

    fn draw_sidebar(&self, f: &mut Frame, area: Rect, wide: bool) {
        let items: Vec<ListItem> = self
            .sessions
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let sel = i == self.sel;
                let mark = if sel { "▸" } else { " " };
                let head = Line::from(vec![
                    Span::styled(format!("{mark}{} ", i + 1), Style::new().fg(DIM)),
                    Span::styled(s.tree.name(), Style::new().add_modifier(if sel { Modifier::BOLD } else { Modifier::empty() })),
                ]);
                let it = if wide {
                    let sub = Line::from(Span::styled(format!("   {}", s.tree.blurb()), Style::new().fg(DIM)));
                    ListItem::new(vec![head, sub])
                } else {
                    ListItem::new(head)
                };
                if sel { it.style(Style::new().bg(Color::Rgb(44, 50, 84))) } else { it }
            })
            .collect();
        f.render_widget(List::new(items).block(Self::block("Trees", self.mode == Mode::Normal)), area);
    }

    fn draw_canvas(&mut self, f: &mut Frame, area: Rect) {
        let s = &self.sessions[self.sel];
        let step = s.steps[s.cur].clone();
        let title = s.tree.name();
        let block = Self::block(title, false).title_bottom(Line::from(Span::styled(
            format!(" step {}/{} {}", s.cur + 1, s.steps.len(), if self.playing { "▶ " } else { "" }),
            Style::new().fg(DIM),
        )));
        let inner = block.inner(area);
        f.render_widget(block, area);
        let parts = Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).split(inner);
        let view = parts[0];
        let (aw, ah) = (view.width as usize, view.height as usize);

        let mut lines: Vec<Line> = vec![];
        if let Some(root) = &step.root {
            let r = render(root, false);
            let (gw, gh) = (r.grid.first().map_or(0, |g| g.len()), r.grid.len());
            if self.follow {
                if let Some((hx, hy, hw)) = r.hot {
                    if hx < self.ox {
                        self.ox = hx.saturating_sub(2);
                    } else if hx + hw > self.ox + aw {
                        self.ox = (hx + hw + 2).saturating_sub(aw);
                    }
                    if hy < self.oy {
                        self.oy = hy.saturating_sub(1);
                    } else if hy + 2 > self.oy + ah {
                        self.oy = (hy + 2).saturating_sub(ah);
                    }
                }
            }
            self.ox = self.ox.min(gw.saturating_sub(aw));
            self.oy = self.oy.min(gh.saturating_sub(ah));
            let pad_left = aw.saturating_sub(gw) / 2;
            let pad_top = if gh < ah { 1 } else { 0 };
            for row in 0..ah {
                if row < pad_top {
                    lines.push(Line::default());
                    continue;
                }
                let gy = self.oy + row - pad_top;
                let Some(cells) = r.grid.get(gy) else { break };
                let end = (self.ox + aw.saturating_sub(pad_left)).min(gw);
                let mut spans = vec![Span::raw(" ".repeat(pad_left))];
                spans.extend(grid_line(&cells[self.ox.min(end)..end]));
                lines.push(Line::from(spans));
            }
        } else {
            lines.push(Line::default());
            lines.push(Line::from(Span::styled("  (empty)", Style::new().fg(DIM))));
        }
        f.render_widget(Paragraph::new(lines), view);

        let info = vec![
            Line::from(Span::styled(step.msg.clone(), Style::new().fg(Color::Rgb(250, 204, 21)).add_modifier(Modifier::BOLD))),
            Line::from(Span::styled(step.aux.clone(), Style::new().fg(DIM))),
        ];
        f.render_widget(Paragraph::new(info).wrap(Wrap { trim: true }), parts[1]);
    }

    fn draw_lesson(&mut self, f: &mut Frame, area: Rect) {
        let pages = self.sessions[self.sel].tree.lesson();
        let page = self.page.min(pages.len() - 1);
        let (name, body) = pages[page];
        let title = format!("Lesson {}/{} · {}   [ ] page", page + 1, pages.len(), name);
        let lines: Vec<Line> = body
            .lines()
            .map(|l| {
                if let Some(h) = l.strip_prefix("## ") {
                    Line::from(Span::styled(h.to_string(), Style::new().fg(GOOD).add_modifier(Modifier::BOLD)))
                } else if let Some(rest) = l.strip_prefix("• ") {
                    Line::from(vec![Span::styled("• ", Style::new().fg(ACCENT)), Span::raw(rest.to_string())])
                } else {
                    Line::raw(l.to_string())
                }
            })
            .collect();
        let p = Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((self.lscroll, 0)).block(Self::block(&title, false));
        f.render_widget(p, area);
    }

    fn draw_events(&self, f: &mut Frame, area: Rect) {
        let s = &self.sessions[self.sel];
        let items: Vec<ListItem> = s
            .steps
            .iter()
            .enumerate()
            .map(|(i, st)| {
                let style = if i == s.cur { Style::new().fg(Color::Rgb(250, 204, 21)) } else { Style::new().fg(DIM) };
                ListItem::new(Line::from(Span::styled(format!("{:>3} {}", i + 1, st.msg), style)))
            })
            .collect();
        let mut state = ListState::default().with_selected(Some(s.cur));
        let list = List::new(items).block(Self::block("Timeline", false)).highlight_style(Style::new().bg(Color::Rgb(44, 50, 84)));
        f.render_stateful_widget(list, area, &mut state);
    }

    fn draw_command(&self, f: &mut Frame, area: Rect) {
        let line = match (&self.mode, &self.status) {
            (Mode::Command, _) => Line::from(vec![
                Span::styled(":", Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)),
                Span::raw(self.input.clone()),
            ]),
            (_, Some((m, true))) => Line::from(Span::styled(format!(" ✗ {m}"), Style::new().fg(BAD))),
            (_, Some((m, false))) => Line::from(Span::styled(format!(" ✓ {m}"), Style::new().fg(GOOD))),
            _ => Line::from(Span::styled(
                format!(" {} · delay {} ms · autoplay {}", self.sessions[self.sel].tree.name(), self.speed_ms, if self.autoplay { "on" } else { "off" }),
                Style::new().fg(DIM),
            )),
        };
        f.render_widget(Paragraph::new(line), area);
        if self.mode == Mode::Command {
            f.set_cursor_position((area.x + 1 + self.input.chars().count() as u16, area.y));
        }
    }

    fn draw_hints(&self, f: &mut Frame, area: Rect) {
        let keys: &[(&str, &str)] = if self.mode == Mode::Command {
            &[("enter", "run"), ("esc", "cancel"), ("↑↓", "history")]
        } else {
            &[("i", "insert"), ("d", "delete"), ("f", "find"), ("r", "random"), ("c", "clear"), ("←→", "step"), ("space", "play"), ("tab", "tree"), ("[ ]", "lesson"), ("?", "help"), ("q", "quit")]
        };
        let mut spans = vec![];
        for (k, d) in keys {
            spans.push(Span::styled(format!(" {k} "), Style::new().fg(Color::Black).bg(ACCENT)));
            spans.push(Span::styled(format!(" {d} "), Style::new().fg(DIM)));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn draw_help(&self, f: &mut Frame, area: Rect) {
        let text = "\
## Keys
  i / d / f        insert / delete / find (type values, Enter)
  r  c             insert random values / clear this tree
  ← →  Home End    step through the timeline
  space            play / pause the timeline
  + -              faster / slower
  a                toggle autoplay of new operations
  Tab ↑ ↓  1-6     switch tree
  [ ]  PgUp PgDn   lesson page / scroll
  h j k l          pan the drawing
  :                command line
  q                quit

## Commands
  insert <v…>   delete <v…>   find <v…>   random [n]   clear
  tree <name>   speed <ms>    autoplay [on|off]
  pop           (heap)  extract the minimum
  complete <p>  (trie)  list words with a prefix

Press any key to close.";
        let lines: Vec<Line> = text
            .lines()
            .map(|l| match l.strip_prefix("## ") {
                Some(h) => Line::from(Span::styled(h.to_string(), Style::new().fg(GOOD).add_modifier(Modifier::BOLD))),
                None => Line::raw(l.to_string()),
            })
            .collect();
        let (w, h) = (68.min(area.width), (lines.len() as u16 + 2).min(area.height));
        let r = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
        f.render_widget(Clear, r);
        f.render_widget(Paragraph::new(lines).block(Self::block("Help", true)), r);
    }
}

// ───────────────────────────── 6. main ─────────────────────────────

const USAGE: &str = "\
arvore — learn tree data structures in your terminal

USAGE
  arvore                       open the interactive TUI
  arvore show <tree> [values]  print a tree built from the values (pipe-friendly)
  arvore ls                    list the available trees
  arvore --help | --version

TREES
  bst  avl  rb  btree  trie  heap";

fn cmd_show(args: &[String]) -> i32 {
    let Some(name) = args.first() else {
        eprintln!("arvore: show needs a tree name ({})", TREE_NAMES.join("|"));
        return 2;
    };
    let Some(mut tree) = make_tree(name) else {
        eprintln!("arvore: unknown tree '{name}' (try: {})", TREE_NAMES.join(", "));
        return 2;
    };
    for v in &args[1..] {
        if let Err(e) = tree.insert(v) {
            eprintln!("arvore: {e}");
            return 1;
        }
    }
    let step = tree.snapshot("");
    match &step.root {
        Some(root) => {
            for row in render(root, true).grid {
                let line: String = row.iter().map(|c| c.0).collect();
                println!("{}", line.trim_end());
            }
        }
        None => println!("(empty)"),
    }
    if !step.aux.is_empty() {
        println!("{}", step.aux);
    }
    0
}

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => {
            if !io::stdout().is_terminal() {
                eprintln!("arvore: the TUI needs a terminal; try `arvore show avl 1 2 3`");
                std::process::exit(2);
            }
            let mut terminal = ratatui::init();
            let res = App::new().run(&mut terminal);
            ratatui::restore();
            res
        }
        Some("show") => std::process::exit(cmd_show(&args[1..])),
        Some("ls") => {
            for n in TREE_NAMES {
                if let Some(t) = make_tree(n) {
                    println!("{:<6} {:<10} {}", n, t.name(), t.blurb());
                }
            }
            Ok(())
        }
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            Ok(())
        }
        Some("-V" | "--version") => {
            println!("arvore {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(other) => {
            eprintln!("arvore: unknown command '{other}'\n\n{USAGE}");
            std::process::exit(2);
        }
    }
}

// ───────────────────────────── tests ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn fuzz_bin(kind: Kind) {
        let mut rng = Rng(0x9E3779B97F4A7C15);
        let mut t = BinTree::new(kind);
        let mut model = BTreeSet::new();
        for _ in 0..4000 {
            let k = rng.range(120) as i64;
            if rng.range(100) < 55 {
                assert_eq!(t.insert(&k.to_string()).is_ok(), model.insert(k));
            } else {
                assert_eq!(t.delete(&k.to_string()).is_ok(), model.remove(&k));
            }
            assert_eq!(t.check(), model.len());
        }
    }

    #[test]
    fn bst_matches_model() {
        fuzz_bin(Kind::Bst);
    }
    #[test]
    fn avl_stays_balanced() {
        fuzz_bin(Kind::Avl);
    }
    #[test]
    fn red_black_invariants() {
        fuzz_bin(Kind::Rb);
    }

    #[test]
    fn avl_sorted_input_is_log_height() {
        let mut t = BinTree::new(Kind::Avl);
        for k in 1..=1023 {
            t.insert(&k.to_string()).unwrap();
        }
        assert!(t.n[t.root].h <= 11, "height {}", t.n[t.root].h);
    }

    #[test]
    fn heap_matches_model() {
        let mut rng = Rng(12345);
        let mut h = Heap::new();
        let mut model: Vec<i64> = vec![];
        for _ in 0..3000 {
            let k = rng.range(60) as i64;
            match rng.range(3) {
                0 => {
                    h.insert(&k.to_string()).unwrap();
                    model.push(k);
                }
                1 => {
                    let r = h.delete(&k.to_string());
                    match model.iter().position(|&v| v == k) {
                        Some(i) => {
                            model.remove(i);
                            assert!(r.is_ok());
                        }
                        None => assert!(r.is_err()),
                    }
                }
                _ => {
                    let r = h.special("pop", &[]).unwrap();
                    match model.iter().min().copied() {
                        Some(m) => {
                            assert!(r.is_ok());
                            model.remove(model.iter().position(|&v| v == m).unwrap());
                        }
                        None => assert!(r.is_err()),
                    }
                }
            }
            h.check();
            assert_eq!(h.a.len(), model.len());
        }
    }

    #[test]
    fn btree_matches_model() {
        let mut rng = Rng(777);
        let mut t = BTree::new();
        let mut model = BTreeSet::new();
        for _ in 0..4000 {
            let k = rng.range(150) as i64;
            if rng.range(100) < 55 {
                assert_eq!(t.insert(&k.to_string()).is_ok(), model.insert(k));
            } else {
                assert_eq!(t.delete(&k.to_string()).is_ok(), model.remove(&k));
            }
            let keys = if t.n[t.root].keys.is_empty() { vec![] } else { t.check() };
            assert_eq!(keys, model.iter().copied().collect::<Vec<_>>());
        }
    }

    #[test]
    fn trie_matches_model() {
        let mut rng = Rng(2024);
        let mut t = Trie::new();
        let mut model = BTreeSet::new();
        for _ in 0..2000 {
            let w = WORDS[rng.range(WORDS.len() as u64) as usize];
            if rng.range(100) < 55 {
                assert_eq!(t.insert(w).is_ok(), model.insert(w.to_string()));
            } else {
                assert_eq!(t.delete(w).is_ok(), model.remove(w));
            }
            assert_eq!(t.words(), model.iter().cloned().collect::<Vec<_>>());
        }
    }

    #[test]
    fn every_step_renders() {
        for name in TREE_NAMES {
            let mut t = make_tree(name).unwrap();
            let mut rng = Rng(5);
            for _ in 0..60 {
                let tok = t.random_token(&mut rng);
                for s in t.insert(&tok).unwrap_or_default() {
                    if let Some(root) = &s.root {
                        let r = render(root, true);
                        assert!(!r.grid.is_empty());
                    }
                }
            }
        }
    }
}
