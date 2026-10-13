import { test } from 'node:test';
import assert from 'node:assert/strict';
import { acquireCallMicrophone } from '../src/callMicrophone.ts';
const install = (getUserMedia) => Object.defineProperty(globalThis, 'navigator', { configurable: true, value: { mediaDevices: { getUserMedia } } });
const stream = () => ({ getTracks: () => [] });
test('retries a transient source failure once', async () => {
    let calls = 0;
    const media = stream();
    install(async () => { if (++calls === 1) throw new DOMException('Could not start audio source', 'NotReadableError'); return media; });
    assert.equal(await acquireCallMicrophone(() => true), media);
    assert.equal(calls, 2);
});
test('does not retry denied permission', async () => {
    let calls = 0;
    install(async () => { calls++; throw new DOMException('Denied', 'NotAllowedError'); });
    await assert.rejects(acquireCallMicrophone(() => true), /Allow microphone/);
    assert.equal(calls, 1);
});
test('serializes requests and releases audio from a cancelled call', async () => {
    let complete;
    let active = true, calls = 0, stopped = 0;
    const next = stream();
    install(() => { calls++; return calls === 1 ? new Promise(resolve => { complete = resolve; }) : Promise.resolve(next); });
    const first = acquireCallMicrophone(() => active);
    await Promise.resolve();
    active = false;
    const second = acquireCallMicrophone(() => true);
    await Promise.resolve();
    assert.equal(calls, 1);
    complete({ getTracks: () => [{ stop() { stopped++; } }] });
    assert.equal(await first, null);
    assert.equal(await second, next);
    assert.equal(stopped, 1);
    assert.equal(calls, 2);
});
test('does not retry after the call ends during recovery', async () => {
    let active = true, calls = 0;
    install(async () => { calls++; active = false; throw new DOMException('Unavailable', 'AbortError'); });
    assert.equal(await acquireCallMicrophone(() => active), null);
    assert.equal(calls, 1);
});
test('persistent source failure stops after two attempts', async () => {
    let calls = 0;
    install(async () => { calls++; throw new DOMException('Unavailable', 'NotReadableError'); });
    await assert.rejects(acquireCallMicrophone(() => true), /Microphone could not start/);
    assert.equal(calls, 2);
});
