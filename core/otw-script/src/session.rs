//! Pine's realtime model (spec §4.2): confirmed bars run once; the forming bar re-runs on every
//! update from the state after the last confirmed bar (rollback) and is committed on close.

use std::rc::Rc;

use crate::compile::Program;
use crate::diag::{RtCode, RuntimeError, Span};
use crate::input::{Bars, RunInput};
use crate::output::RunOutput;
use crate::vm::{check_bars, BarMode, Machine};

pub struct Session {
    program: Rc<Program>,
    input: RunInput,
    /// State after the last confirmed bar.
    snapshot: Machine,
    out: RunOutput,
    committed: usize,
    /// Open time of the forming bar, when there is one.
    forming: Option<i64>,
}

impl Session {
    /// Run every bar of `input`; the last one as the forming bar when `forming`.
    pub fn open(program: Rc<Program>, input: RunInput, forming: bool) -> Result<Session, RuntimeError> {
        check_bars(&input.bars)?;
        let n = input.bars.len();
        let forming = forming && n > 0;
        let hist = if forming { n - 1 } else { n };
        let mut out = crate::stdlib::plot::empty(&program.out_counts, n);
        let mut m = Machine::new(&program);
        m.steps(&program, &input, &mut out, 0..hist, hist.checked_sub(1), |_| BarMode::History)?;
        let snapshot = m.clone();
        if forming {
            m.steps(&program, &input, &mut out, hist..n, hist.checked_sub(1), |_| BarMode::Forming { first: true })?;
        }
        let forming = forming.then(|| input.bars.time[n - 1]);
        Ok(Session { program, input, snapshot, out, committed: hist, forming })
    }

    /// `tail` = the bars from index `committed()` on, replacing any forming bar. All run as
    /// committed except the last when `forming`. On error the session is unchanged.
    pub fn advance(&mut self, tail: Bars, forming: bool) -> Result<(), RuntimeError> {
        check_bars(&tail)?;
        if self.input.bars.volume.is_empty() != tail.volume.is_empty() && !tail.is_empty() {
            return Err(RuntimeError { code: RtCode::BadBars, span: Span::default(), bar: self.committed, args: vec!["volume".into()] });
        }
        let c = self.committed;
        let k = tail.len();
        let forming = forming && k > 0;
        let n_commit = if forming { k - 1 } else { k };
        let len = c + k;

        let saved_bars = cut(&mut self.input.bars, c);
        append(&mut self.input.bars, tail);
        let saved_out = self.out.columns_from(c);
        let saved_drawings = self.out.drawings.clone();
        self.out.reset_from(c, len);

        let mut m = self.snapshot.clone();
        let was = self.forming;
        let time = &self.input.bars.time;
        let mut r = m.steps(&self.program, &self.input, &mut self.out, c..c + n_commit, None, |i| BarMode::Commit {
            was_forming: was == Some(time[i]),
        });
        let committed_state = if r.is_ok() && n_commit > 0 { Some(m.clone()) } else { None };
        if r.is_ok() && forming {
            let j = c + n_commit;
            let first = was != Some(time[j]);
            r = m.steps(&self.program, &self.input, &mut self.out, j..len, None, |_| BarMode::Forming { first });
        }
        if let Err(e) = r {
            cut(&mut self.input.bars, c);
            append(&mut self.input.bars, saved_bars);
            self.out.reset_from(c, c);
            self.out.splice_columns(c, saved_out);
            self.out.drawings = saved_drawings;
            return Err(e);
        }
        if let Some(s) = committed_state {
            self.snapshot = s;
        }
        self.committed = c + n_commit;
        self.forming = forming.then(|| self.input.bars.time[len - 1]);
        Ok(())
    }

    /// Bars run as confirmed.
    pub fn committed(&self) -> usize {
        self.committed
    }

    /// Bars run, the forming one included.
    pub fn len(&self) -> usize {
        self.input.bars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.input.bars.is_empty()
    }

    pub fn output(&self) -> &RunOutput {
        &self.out
    }

    /// Per-bar columns from bar `from` on; per-run fields (hline price, linewidth, …) whole.
    pub fn output_from(&self, from: usize) -> RunOutput {
        self.out.columns_from(from)
    }
}

/// Remove and return bars `from..` of `b`.
fn cut(b: &mut Bars, from: usize) -> Bars {
    fn tail<T>(v: &mut Vec<T>, from: usize) -> Vec<T> {
        if from < v.len() {
            v.split_off(from)
        } else {
            Vec::new()
        }
    }
    Bars {
        time: tail(&mut b.time, from),
        time_close: tail(&mut b.time_close, from),
        open: tail(&mut b.open, from),
        high: tail(&mut b.high, from),
        low: tail(&mut b.low, from),
        close: tail(&mut b.close, from),
        volume: tail(&mut b.volume, from),
    }
}

fn append(b: &mut Bars, t: Bars) {
    b.time.extend(t.time);
    b.time_close.extend(t.time_close);
    b.open.extend(t.open);
    b.high.extend(t.high);
    b.low.extend(t.low);
    b.close.extend(t.close);
    b.volume.extend(t.volume);
}
