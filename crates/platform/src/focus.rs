use std::sync::Mutex;

use said_core::{Result, Target};

#[derive(Default)]
pub struct FocusQueue(Mutex<Vec<Option<Target>>>);

impl FocusQueue {
    pub fn push(&self, target: Option<Target>) {
        self.0.lock().unwrap().push(target);
    }

    pub fn drain(&self) -> Vec<Option<Target>> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }

    pub fn post(
        &self,
        target: &Target,
        automatic: bool,
        post: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        let events = self.0.lock().unwrap();
        if automatic
            && events.iter().any(|event| {
                !event
                    .as_ref()
                    .is_some_and(|current| current.same_window(target))
            })
        {
            return Err("Focus moved. Text is saved; use Insert last or paste manually.".into());
        }
        post()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn queued_departure_and_return_withhold_automatic_input_but_allow_explicit_insert() {
        let target = Target {
            app_id: "scratch".into(),
            window_id: "A".into(),
            name: "Scratch".into(),
        };
        for departure in [
            None,
            Some(Target {
                window_id: "B".into(),
                ..target.clone()
            }),
        ] {
            let queue = FocusQueue::default();
            queue.push(departure);
            queue.push(Some(target.clone()));
            let posted = Cell::new(false);
            assert!(
                queue
                    .post(&target, true, || {
                        posted.set(true);
                        Ok(())
                    })
                    .is_err()
            );
            assert!(!posted.get());
            assert_eq!(queue.drain().len(), 2);
            queue.push(None);
            queue
                .post(&target, false, || {
                    posted.set(true);
                    Ok(())
                })
                .unwrap();
            assert!(posted.get());
        }
    }

    #[test]
    fn unchanged_focus_allows_automatic_post() {
        let target = Target {
            app_id: "scratch".into(),
            window_id: "A".into(),
            name: "Scratch".into(),
        };
        let queue = FocusQueue::default();
        queue.push(Some(target.clone()));
        queue.post(&target, true, || Ok(())).unwrap();
    }
}
