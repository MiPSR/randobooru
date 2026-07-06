use std::{sync::Arc, time::Duration};

use tokio::{sync::Mutex, time::sleep};

#[derive(Clone)]
pub struct ApiPacer {
	interval: Duration,
	next_allowed: Arc<Mutex<std::time::Instant>>,
}

impl ApiPacer {
	pub fn new(interval: Duration) -> Self {
		Self {
			interval,
			next_allowed: Arc::new(Mutex::new(std::time::Instant::now())),
		}
	}

	pub async fn wait(&self) {
		if self.interval.is_zero() {
			return;
		}

		let sleep_for = {
			let mut next_allowed = self.next_allowed.lock().await;
			let now = std::time::Instant::now();

			if *next_allowed <= now {
				*next_allowed = now + self.interval;
				None
			} else {
				let sleep_for = *next_allowed - now;
				*next_allowed += self.interval;
				Some(sleep_for)
			}
		};

		if let Some(duration) = sleep_for {
			sleep(duration).await;
		}
	}
}
