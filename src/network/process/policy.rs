//! When the supervisor reports, retries, or gives up: a pure state machine
//! over a clock the caller passes in, from when it started supervising.
//! `forward.rs` runs the processes.

use std::time::Duration;

pub(super) const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);
pub(super) const INITIAL_RESTART_BACKOFF: Duration = Duration::from_millis(250);
pub(super) const MAX_RESTART_BACKOFF: Duration = Duration::from_secs(5);

/// After a child that did not listen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Step {
    /// Spawn again after this delay.
    Retry(Duration),
    /// Tell the caller it failed, and stop.
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct Policy {
    reported: bool,
    backoff: Duration,
}

impl Policy {
    pub(super) fn new() -> Self {
        Self {
            reported: false,
            backoff: INITIAL_RESTART_BACKOFF,
        }
    }

    /// The child could not spawn, or exited or timed out before it listened.
    /// A retry that would start past the startup deadline could not listen
    /// in time, and its empty log would replace this child's error.
    pub(super) fn not_ready(&mut self, now: Duration) -> Step {
        if !self.reported && now + self.backoff >= STARTUP_TIMEOUT {
            return Step::Fail;
        }
        Step::Retry(self.retry())
    }

    /// True the first time: the caller hears the tunnel is up.
    pub(super) fn ready(&mut self) -> bool {
        self.backoff = INITIAL_RESTART_BACKOFF;
        !std::mem::replace(&mut self.reported, true)
    }

    /// A ready child exited on its own. Returns the delay before a restart.
    pub(super) fn exited(&mut self) -> Duration {
        self.retry()
    }

    /// How long the next child may take to listen. A restart after
    /// readiness gets a full budget: the startup deadline is long gone.
    pub(super) fn listen_deadline(&self, now: Duration) -> Duration {
        if self.reported {
            now + STARTUP_TIMEOUT
        } else {
            STARTUP_TIMEOUT
        }
    }

    /// The owner dropped the tunnel. True when the caller still waits to
    /// hear, so it must be told the start failed.
    pub(super) fn shutdown(&self) -> bool {
        !self.reported
    }

    fn retry(&mut self) -> Duration {
        let delay = self.backoff;
        self.backoff = (self.backoff * 2).min(MAX_RESTART_BACKOFF);
        delay
    }

    pub(super) fn reported(&self) -> bool {
        self.reported
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashSet, VecDeque};

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum Event {
        NotReady,
        Ready,
        Exited,
        Shutdown,
        Advance(Duration),
    }

    /// What the supervisor loop can observe next.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum Child {
        Starting,
        Listening,
    }

    /// What the supervisor does about an event.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Out {
        Retry(Duration),
        Report,
        Fail,
        Nothing,
    }

    fn events(child: Child) -> Vec<Event> {
        let mut events = vec![
            Event::Shutdown,
            Event::Advance(Duration::from_secs(1)),
            // Lands just short of the deadline, where a retry no longer fits.
            Event::Advance(STARTUP_TIMEOUT - Duration::from_secs(2)),
            Event::Advance(STARTUP_TIMEOUT),
        ];
        match child {
            Child::Starting => events.extend([Event::NotReady, Event::Ready]),
            Child::Listening => events.push(Event::Exited),
        }
        events
    }

    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    struct State {
        policy: Policy,
        child: Child,
        now: Duration,
        reports: u8,
        fails: u8,
        /// The supervisor returned: it failed, or its owner stopped it.
        stopped: bool,
    }

    fn step(state: &State, event: Event) -> (State, Out) {
        let mut next = state.clone();
        let out = match event {
            Event::Advance(by) => {
                next.now += by;
                Out::Nothing
            }
            Event::NotReady => match next.policy.not_ready(next.now) {
                Step::Retry(delay) => Out::Retry(delay),
                Step::Fail => Out::Fail,
            },
            Event::Ready => {
                next.child = Child::Listening;
                if next.policy.ready() {
                    Out::Report
                } else {
                    Out::Nothing
                }
            }
            Event::Exited => {
                next.child = Child::Starting;
                Out::Retry(next.policy.exited())
            }
            Event::Shutdown => {
                if next.policy.shutdown() {
                    Out::Fail
                } else {
                    Out::Nothing
                }
            }
        };
        match out {
            Out::Report => next.reports += 1,
            Out::Fail => next.fails += 1,
            Out::Retry(_) | Out::Nothing => {}
        }
        next.stopped |= out == Out::Fail || event == Event::Shutdown;
        (next, out)
    }

    fn check(before: &State, event: Event, out: Out, after: &State) -> Result<(), String> {
        if after.reports > 1 {
            return Err("reported ready twice".to_string());
        }
        if after.fails > 1 {
            return Err("reported failure twice".to_string());
        }
        if after.reports == 1 && after.fails == 1 {
            return Err("reported both ready and failed".to_string());
        }
        let budget = after
            .policy
            .listen_deadline(after.now)
            .saturating_sub(after.now);
        if after.policy.reported() && budget < STARTUP_TIMEOUT {
            return Err(format!("a restart only gets {budget:?} to listen"));
        }
        if !after.policy.reported() && after.policy.listen_deadline(after.now) != STARTUP_TIMEOUT {
            return Err("startup budget moved before readiness".to_string());
        }
        match out {
            Out::Retry(delay) => {
                if !(INITIAL_RESTART_BACKOFF..=MAX_RESTART_BACKOFF).contains(&delay) {
                    return Err(format!("retry delay {delay:?} out of bounds"));
                }
                if event == Event::NotReady
                    && !before.policy.reported()
                    && before.now + delay >= STARTUP_TIMEOUT
                {
                    return Err("a retry starts past the startup deadline".to_string());
                }
                if before.policy.reported()
                    && before.child == Child::Listening
                    && delay != INITIAL_RESTART_BACKOFF
                {
                    return Err(
                        "first restart after readiness did not reset the backoff".to_string()
                    );
                }
            }
            // Never give up while a retry can still start in the budget.
            Out::Fail
                if event == Event::NotReady
                    && before.now + before.policy.backoff < STARTUP_TIMEOUT =>
            {
                return Err("gave up before the startup deadline".to_string());
            }
            Out::Fail if before.policy.reported() => {
                return Err("failure after readiness".to_string());
            }
            Out::Report if event != Event::Ready => {
                return Err("reported ready without a ready child".to_string());
            }
            // A caller still waiting always hears how the start ended.
            Out::Nothing if event == Event::Shutdown && !before.policy.reported() => {
                return Err("stopped without telling the waiting caller".to_string());
            }
            _ => {}
        }
        Ok(())
    }

    #[test]
    fn every_supervisor_history_keeps_the_invariants() {
        let start = State {
            policy: Policy::new(),
            child: Child::Starting,
            now: Duration::ZERO,
            reports: 0,
            fails: 0,
            stopped: false,
        };
        let mut seen = HashSet::from([start.clone()]);
        let mut queue = VecDeque::from([(start, 0usize)]);
        while let Some((state, depth)) = queue.pop_front() {
            // A stopped supervisor gets no more events.
            if depth == 14 || state.stopped {
                continue;
            }
            for event in events(state.child) {
                let (next, out) = step(&state, event);
                if let Err(why) = check(&state, event, out, &next) {
                    panic!("{why} on {event:?} from {state:?}");
                }
                if seen.insert(next.clone()) {
                    queue.push_back((next, depth + 1));
                }
            }
        }
        assert!(seen.len() > 1_500, "explored only {} states", seen.len());
    }

    #[test]
    fn backoff_doubles_up_to_the_cap() {
        let mut policy = Policy::new();
        let delays = (0..8)
            .map(|_| policy.not_ready(Duration::ZERO))
            .collect::<Vec<_>>();
        assert_eq!(delays[0], Step::Retry(INITIAL_RESTART_BACKOFF));
        assert_eq!(delays[1], Step::Retry(INITIAL_RESTART_BACKOFF * 2));
        assert_eq!(delays[7], Step::Retry(MAX_RESTART_BACKOFF));
    }

    #[test]
    fn a_restart_an_hour_in_gets_the_full_startup_budget() {
        let mut policy = Policy::new();
        assert!(policy.ready());
        let hour = Duration::from_secs(3600);
        assert_eq!(policy.exited(), INITIAL_RESTART_BACKOFF);
        assert_eq!(policy.listen_deadline(hour), hour + STARTUP_TIMEOUT);
    }

    #[test]
    fn startup_fails_once_no_retry_can_start_before_the_deadline() {
        let mut policy = Policy::new();
        let last_chance = STARTUP_TIMEOUT - INITIAL_RESTART_BACKOFF - Duration::from_millis(1);
        assert_eq!(
            policy.not_ready(last_chance),
            Step::Retry(INITIAL_RESTART_BACKOFF)
        );
        assert_eq!(policy.not_ready(last_chance), Step::Fail);
    }
}
