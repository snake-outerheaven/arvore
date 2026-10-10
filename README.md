<div align="center">

# 🌳 arvore

**Watch tree data structures think.**
A terminal playground where you insert, delete and search — and see every rotation, recolor, split and merge, one step at a time.

[![CI](https://github.com/snake-outerheaven/arvore/actions/workflows/ci.yml/badge.svg)](https://github.com/snake-outerheaven/arvore/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)
![Rust](https://img.shields.io/badge/rust-2024-orange.svg)
![ratatui](https://img.shields.io/badge/TUI-ratatui-blueviolet.svg)

</div>

---

Textbooks show you the *before* and the *after* of a red-black insert. **arvore** shows you the part in the middle — and lets you rewind it.

```
            4(B)
      ┌───────┴───────┐
    2(B)            6(B)
   ┌──┴───┐      ┌────┴────┐
 1(B)   3(B)   5(B)      8(R)
                        ┌──┴───┐
                      7(B)   9(B)
                               └──┐
                                10(R)
```
<sub>`arvore show rb 1 2 3 4 5 6 7 8 9 10` — the same drawing, as plain text.</sub>

## Why you'll like it

- **A timeline, not a snapshot.** Every operation records each comparison, rotation, recolor, split and merge. Step forward *and backward* with `←` `→`, or hit `space` and watch it play.
- **The tree explains itself.** Each frame carries a one-line reason: *“uncle is black: recolor 4 black, 6 red, then rotate right at 6”*. The affected nodes light up.
- **Six trees, one interface.** Compare how the same keys behave in a BST, an AVL and a red-black tree. Insert `1 2 3 4 5 6 7` in each and see why balancing matters.
- **Built-in lessons.** Every tree has a lesson pane: the invariants, the cases, the complexity, when (not) to use it.
- **Correctness you can trust.** Every structure is fuzz-tested against a reference model, checking its invariants after *every* operation (red-black rules, AVL balance, B-tree depth and fill, heap order…).
- **One file, one binary.** ~2200 lines of Rust in a single `main.rs`. Easy to read, easy to hack on.

## The trees

| Tree | What you'll see |
|---|---|
| **BST** | the plain ordered tree — and how sorted input degrades it into a list |
| **AVL** | balance factors on every node; the four rotation cases (LL, RR, LR, RL) |
| **Red-Black** | red/black recoloring, uncle cases, double-black repair on delete |
| **B-Tree** (t = 2) | node splits on the way down, borrowing and merging on delete |
| **Trie** | shared prefixes, end-of-word marks, pruning, `:complete` autocomplete |
| **Heap** | the tree *and* the array behind it; sift-up / sift-down, `:pop` |

## Install & run

You need [Rust](https://rustup.rs) (edition 2024).

```bash
git clone https://github.com/snake-outerheaven/arvore
cd arvore
cargo run --release
```

Then press `i`, type `5 3 8 1 4 7`, and hit Enter.

Not in a TUI mood? It also works in pipes:

```bash
arvore show avl 1 2 3 4 5 6 7   # plain-text drawing
arvore ls                       # list the trees
```

## Keys

| Key | Action |
|---|---|
| `i` `d` `f` | insert / delete / find (type values, Enter) |
| `r` `c` | insert random values / clear the tree |
| `←` `→` `Home` `End` | step through the timeline |
| `space` | play / pause |
| `+` `-` | faster / slower |
| `a` | toggle autoplay of new operations |
| `Tab` `↑` `↓` `1`–`6` | switch tree |
| `[` `]` | previous / next lesson page |
| `h` `j` `k` `l` | pan the drawing |
| `:` | command line (`insert 1 2 3`, `random 20`, `tree avl`, `speed 300`, `pop`, `complete ca`) |
| `?` | help · `q` quit |

## Try this

1. **Why balance matters.** On BST: `:insert 1 2 3 4 5 6 7`. Switch to AVL and repeat. Compare the shapes.
2. **The four AVL cases.** On AVL, one at a time: `:insert 1 2 3`, `:insert 3 2 1`, `:insert 3 1 2`, `:insert 1 3 2`.
3. **Cascading recolor.** On Red-Black: `:insert 1 2 3 4 5 6 7 8`, then step back through the timeline.
4. **Watch a B-tree grow.** On B-Tree: `:insert 10 20 30 40 50 60 70`, then `:delete 10 20 30`.
5. **Autocomplete.** On Trie: `:insert car cart care cat dog`, then `:complete ca`.

## Under the hood

`src/main.rs` reads top to bottom in six sections: the drawable model, the trees (BST/AVL/Red-Black share one arena with a NIL sentinel, CLRS-style), the layout engine that turns any tree into a box-drawing grid, the lessons, the ratatui app, and the CLI entry points. Each tree records a `Step` after every interesting event; the UI just scrubs through that list.

```bash
cargo test    # fuzzes every structure and checks its invariants
```

## Roadmap

- [ ] Splay, Treap, Segment tree, Fenwick tree
- [ ] `quiz` mode: predict the next rotation
- [ ] `bench`: compare trees on real workloads
- [ ] Traversal animations (pre/in/post-order, BFS)
- [ ] Configurable B-tree order

Ideas and PRs welcome.

## License

[MIT](LICENSE) © neo-dev
