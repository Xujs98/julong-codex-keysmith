const DEFAULT_TOLERANCE = 2;

function finiteNumber(value) {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? Math.max(0, parsed) : 0;
}

export function getScrollGateState(metrics, tolerance = DEFAULT_TOLERANCE) {
    const scrollTop = finiteNumber(metrics?.scrollTop);
    const clientHeight = finiteNumber(metrics?.clientHeight);
    const scrollHeight = finiteNumber(metrics?.scrollHeight);
    const threshold = finiteNumber(tolerance);
    const overflow = Math.max(0, scrollHeight - clientHeight);
    const remaining = Math.max(0, overflow - scrollTop);
    const requiresScroll = overflow > threshold;

    return {
        requiresScroll,
        unlocked: !requiresScroll || remaining <= threshold,
        remaining,
    };
}

export function syncScrollGate(scroller, button, status, tolerance = DEFAULT_TOLERANCE) {
    const state = getScrollGateState(scroller, tolerance);
    button.disabled = !state.unlocked;
    button.setAttribute('aria-disabled', String(!state.unlocked));
    scroller.dataset.scrollGate = state.unlocked ? 'complete' : 'pending';

    if (status) {
        status.dataset.state = state.unlocked ? 'complete' : 'pending';
        status.textContent = state.unlocked
            ? (state.requiresScroll ? '已查看全部变更，可以继续' : '全部变更已显示，可以继续')
            : '请向下滚动并查看全部变更';
    }

    return state;
}

export function createScrollGate({ scroller, button, status, resizeTarget = globalThis, tolerance = DEFAULT_TOLERANCE }) {
    if (!scroller || !button) throw new TypeError('scroll gate requires a scroller and button');

    const update = () => syncScrollGate(scroller, button, status, tolerance);
    const scheduleUpdate = () => {
        const scheduleTarget = resizeTarget && typeof resizeTarget.requestAnimationFrame === 'function'
            ? resizeTarget
            : globalThis;
        const schedule = scheduleTarget?.requestAnimationFrame;
        if (typeof schedule === 'function') schedule.call(scheduleTarget, update);
        else update();
    };

    scroller.addEventListener('scroll', update, { passive: true });
    resizeTarget?.addEventListener?.('resize', scheduleUpdate);

    return {
        update,
        lock() {
            button.disabled = true;
            button.setAttribute('aria-disabled', 'true');
            scroller.dataset.scrollGate = 'pending';
            if (status) {
                status.dataset.state = 'pending';
                status.textContent = '请向下滚动并查看全部变更';
            }
        },
        destroy() {
            scroller.removeEventListener('scroll', update);
            resizeTarget?.removeEventListener?.('resize', scheduleUpdate);
        },
    };
}
