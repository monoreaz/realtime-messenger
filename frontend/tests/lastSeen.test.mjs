import { test } from 'node:test';
import assert from 'node:assert/strict';
import { formatLastSeen, withLastSeen } from '../src/lastSeen.ts';

const now = new Date(2026, 10, 20, 15, 30, 0);
const ago = (ms) => new Date(now.getTime() - ms).toISOString();
const time = (date) => date.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });

test('recent activity uses relative wording', () => {
    assert.equal(formatLastSeen(ago(20_000), now, 'en-GB'), 'Last seen just now');
    assert.equal(formatLastSeen(ago(-5_000), now, 'en-GB'), 'Last seen just now');
    assert.equal(formatLastSeen(ago(60_000), now, 'en-GB'), 'Last seen 1 minute ago');
    assert.equal(formatLastSeen(ago(59 * 60_000), now, 'en-GB'), 'Last seen 59 minutes ago');
});

test('older activity names the day', () => {
    const today = new Date(2026, 10, 20, 9, 5);
    const yesterday = new Date(2026, 10, 19, 23, 45);
    assert.equal(formatLastSeen(today.toISOString(), now, 'en-GB'), `Last seen today at ${time(today)}`);
    assert.equal(formatLastSeen(yesterday.toISOString(), now, 'en-GB'), `Last seen yesterday at ${time(yesterday)}`);
    assert.equal(formatLastSeen(new Date(2026, 10, 3, 12).toISOString(), now, 'en-GB'), 'Last seen 3 Nov');
    assert.equal(formatLastSeen(new Date(2025, 0, 7, 12).toISOString(), now, 'en-GB'), 'Last seen 7 Jan 2025');
});

test('invalid timestamps fall back to Offline', () => {
    assert.equal(formatLastSeen('nope', now), 'Offline');
});

test('withLastSeen sets or clears a user entry without mutating', () => {
    const start = new Map([['a', '2026-11-20T10:00:00Z']]);
    const set = withLastSeen(start, 'b', '2026-11-20T11:00:00Z');
    assert.equal(set.get('b'), '2026-11-20T11:00:00Z');
    assert.equal(start.has('b'), false);
    assert.equal(withLastSeen(set, 'a', null).has('a'), false);
    assert.equal(withLastSeen(set, 'a', undefined).has('a'), false);
});
