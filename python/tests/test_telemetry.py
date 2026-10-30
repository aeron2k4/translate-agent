import io
import json

from xt_common.telemetry import Event, JsonSink, hash_id


def test_hash_id_is_stable_and_hides_raw() -> None:
    a = hash_id(b"k", "thread-123")
    assert a == hash_id(b"k", "thread-123")
    assert "thread-123" not in a
    assert hash_id(b"k2", "thread-123") != a
    assert len(a) == 24


def test_json_sink_drops_empty_fields() -> None:
    buf = io.StringIO()
    JsonSink(buf).emit(Event(name="review.approved", count=1))
    assert json.loads(buf.getvalue()) == {"name": "review.approved", "count": 1}
