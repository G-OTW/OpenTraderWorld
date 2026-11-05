# Memory hygiene

## When to use

When you are about to call `memory_write`, and when the memory index in your context looks
stale, duplicated, or self-contradictory.

## The test

Save a fact only if **all four** hold:

1. **Durable**: still true next month. Not a price, not a run result, not today's plan.
2. **Not already stored elsewhere.** If it lives in the journal, the portfolio, the strategy
   list, or the run history, read it from there. Memory is not a cache, and a stale copy of
   queryable data is worse than no copy.
3. **Cross-conversation.** It would change how you behave in a *different* conversation. A
   detail that only matters to the task in front of you does not qualify.
4. **The user's, not yours.** Preferences, constraints, context about how they work. Not your
   own conclusions: those belong in the reply, or in a document.

Anything failing one of the four: do not save it. The index rides in every system prompt, so
each entry costs tokens on every turn, forever. Twenty sharp memories beat two hundred vague
ones.

## Procedure

1. Check the index in your context first. It lists older memories without their description,
   and past a point not at all: `memory_search` with a word finds the rest. If a memory already
   covers the topic, **update it** (`memory_write` on the same slug) rather than adding a
   near-duplicate under a new slug. A memory the user wrote by hand refuses the overwrite
   (the write returns an error saying so): save under another slug and tell the user, or ask
   them to change theirs.
2. Pick a slug that reads as its topic: `base-currency`, `risk-tolerance`,
   `preferred-timeframes`. Not `note-3`, not a date.
3. Write a description that says what the memory answers: it is all you will see next time
   when deciding whether to open it.
4. Keep the body short and specific. Prefer "trades EURUSD and gold, London session only" over
   "interested in forex".
5. Say in your reply that you saved it, and what it says. Silent memory writes are how a user
   ends up steered by something they never agreed to.

## Persona note

Memory is shared across personas and tagged with the one that wrote it. That is deliberate: a
constraint the user states once should reach all of them. So write facts that hold generally,
and when a fact is only true inside one role's work, say so in the body ("for backtesting:
…") rather than assuming the reader shares your role.

## Contradictions

When a memory conflicts with what the user just told you, the user wins. Update the memory, and
say you did. If the user wrote that memory by hand, the write is refused: say what conflicts and ask the user
to change it. Never argue from a stored fact against a live statement.

Rewrite a falsified memory, do not annotate it: a memory that says "was true until March" is a
trap for the next conversation. You never delete on your own initiative. `memory_delete` is for
when the user asks to forget something, or when the store is full and the user picks what goes;
the user confirms every deletion.

## Verification

After writing, confirm the slug you used. If you meant to update an existing memory but used a
new slug, you have just created a duplicate: say so, and ask the user whether to delete one.
