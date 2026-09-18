import assert from 'node:assert/strict';
import test from 'node:test';

import { getScrollGateState, syncScrollGate } from '../frontend/scroll-gate.mjs';

function fakeElement(metrics = {}) {
    return {
        dataset: {},
        disabled: false,
        attributes: {},
        ...metrics,
        setAttribute(name, value) {
            this.attributes[name] = value;
        },
    };
}

test('unlocks when all content already fits in the viewport', () => {
    assert.deepEqual(getScrollGateState({ scrollTop: 0, clientHeight: 320, scrollHeight: 320 }), {
        requiresScroll: false,
        unlocked: true,
        remaining: 0,
    });
});

test('stays locked until the scroll position reaches the bottom tolerance', () => {
    assert.equal(getScrollGateState({ scrollTop: 279, clientHeight: 320, scrollHeight: 640 }).unlocked, false);
    assert.equal(getScrollGateState({ scrollTop: 318, clientHeight: 320, scrollHeight: 640 }).unlocked, true);
});

test('normalizes invalid and overscrolled layout metrics', () => {
    assert.deepEqual(getScrollGateState({ scrollTop: Number.NaN, clientHeight: -10, scrollHeight: 'bad' }), {
        requiresScroll: false,
        unlocked: true,
        remaining: 0,
    });
    assert.equal(getScrollGateState({ scrollTop: 999, clientHeight: 200, scrollHeight: 400 }).remaining, 0);
});

test('synchronizes disabled, ARIA, and status states', () => {
    const scroller = fakeElement({ scrollTop: 0, clientHeight: 200, scrollHeight: 600 });
    const button = fakeElement();
    const status = fakeElement({ textContent: '' });

    syncScrollGate(scroller, button, status);
    assert.equal(button.disabled, true);
    assert.equal(button.attributes['aria-disabled'], 'true');
    assert.equal(scroller.dataset.scrollGate, 'pending');
    assert.match(status.textContent, /滚动/);

    scroller.scrollTop = 400;
    syncScrollGate(scroller, button, status);
    assert.equal(button.disabled, false);
    assert.equal(button.attributes['aria-disabled'], 'false');
    assert.equal(scroller.dataset.scrollGate, 'complete');
    assert.match(status.textContent, /可以继续/);
});
