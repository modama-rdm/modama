import threading
import time
from collections import OrderedDict, deque


class FailureThrottle:
    """
    Counts recent failures per key and reports when a key is over its limit.

    Deliberately in-process and approximate. It exists to blunt password
    guessing against the login endpoint (ASVS 2.2.1), not to be an accounting
    system: there is one instance of this service, and a throttle that needs a
    shared store would drag in the infrastructure the stateless-session decision
    avoided.

    Only failures are recorded and a success clears the key, so a user typing
    their password wrong twice and then correctly is never affected.
    """

    def __init__(self, max_failures: int, window_seconds: int, max_keys: int = 10_000):
        self._max_failures = max_failures
        self._window_seconds = window_seconds
        self._max_keys = max_keys
        self._lock = threading.Lock()
        # Ordered so the least-recently-touched key can be evicted; without a
        # cap, a flood of distinct usernames would grow this without bound.
        self._failures: OrderedDict[str, deque[float]] = OrderedDict()

    def is_blocked(self, key: str) -> bool:
        with self._lock:
            recent = self._recent_locked(key)
            return len(recent) >= self._max_failures

    def record_failure(self, key: str) -> None:
        with self._lock:
            recent = self._recent_locked(key)
            recent.append(time.monotonic())
            self._failures[key] = recent
            self._failures.move_to_end(key)
            while len(self._failures) > self._max_keys:
                _ = self._failures.popitem(last=False)

    def reset(self, key: str) -> None:
        with self._lock:
            _ = self._failures.pop(key, None)

    def retry_after_seconds(self, key: str) -> int:
        """Whole seconds until the oldest failure in the window ages out."""
        with self._lock:
            recent = self._recent_locked(key)
            if not recent:
                return 0
            elapsed = time.monotonic() - recent[0]
            return max(1, int(self._window_seconds - elapsed) + 1)

    def _recent_locked(self, key: str) -> "deque[float]":
        recent = self._failures.get(key, deque())
        cutoff = time.monotonic() - self._window_seconds
        while recent and recent[0] < cutoff:
            _ = recent.popleft()
        return recent
