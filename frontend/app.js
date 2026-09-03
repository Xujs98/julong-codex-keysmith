// Super-Instruct — 前端事件监听 + 渲染 + Tauri 命令调用

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// ── DOM 引用 ─────────────────────────────

const $ = (id) => document.getElementById(id);

const el = {
    // 导航
    navDashboard:  $('nav-dashboard'),
    navConfig:     $('nav-config'),
    navProviders:  $('nav-providers'),
    navSkills:     $('nav-skills'),
    navLog:        $('nav-log'),
    navToggleProxy: $('nav-toggle-proxy'),
    // 标题栏按钮
    tbMinimize:    $('tb-minimize'),
    tbMaximize:    $('tb-maximize'),
    tbClose:       $('tb-close'),
    // 侧边栏状态
    ssDot:         $('ss-dot'),
    ssProxyStatus: $('ss-proxy-status'),
    ssRelay:       $('ss-relay'),
    ssMemory:      $('ss-memory'),
    // 实时活动中的统计
    statTotal:     $('stat-total'),
    statCrack:     $('stat-crack'),
    statReverse:   $('stat-reverse'),
    statPentest:   $('stat-pentest'),
    statTamper:    $('stat-tamper'),
    // 日志
    logList:       $('log-list'),
    logCount:      $('log-count'),
    btnClearLog:   $('btn-clear-log'),
    // 配置
    btnRefresh:    $('btn-refresh'),
    btnDeploy:     $('btn-deploy'),
    btnRestore:    $('btn-restore'),
    btnRelaySave:  $('btn-save-relay'),
    cfgCodexHome:  $('cfg-codex-home'),
    cfgRelayUrl:   $('cfg-relay-url'),
    cfgRelayInput: $('cfg-relay-input'),
    cfgRelayMsg:   $('cfg-relay-message'),
    cfgBridgeStatus: $('cfg-bridge-status'),
    cfgIntegrityStatus: $('cfg-integrity-status'),
    cfgTransactionStatus: $('cfg-transaction-status'),
    cfgMessage:    $('cfg-message'),
    cfgMemoryCount: $('cfg-memory-count'),
    overviewStatus: $('overview-status'),
    overviewStatusMeta: $('overview-status-meta'),
    overviewIntegrity: $('overview-integrity'),
    overviewIntegrityMeta: $('overview-integrity-meta'),
    overviewMemory: $('overview-memory'),
    providersList: $('providers-list'),
    providerCount: $('nav-provider-count'),
    providerRuntime: $('provider-runtime'),
};

// ── 状态 ────────────────────────────────

let isRunning = false;
// 防双击: invoke 进行中置灰开关，避免秒启秒停导致配置目录反复 deploy/restore
let proxyBusy = false;
let stopConfirmTimer = null;
let stopConfirmRemaining = 0;
let stopConfirmPreviousFocus = null;
let logEntries = 0;
let logEvents = [];
let logFilter = 'all';
let logPage = 1;
const logPageSize = 8;

// 类别中文映射
const categoryMap = {
    crack:   '破解',
    reverse: '逆向',
    pentest: '渗透',
    general: '通用',
    system:  '系统',
};

const skillNameMap = {
    'anti-debug':'反调试检测', 'binary-protect-bypass':'二进制保护分析', 'card-key':'卡密生成与校验',
    'cloud-audit-bypass':'云审计路由', 'code-obfuscate':'代码混淆分析', 'crack-keygen':'授权与密钥分析',
    'crypto-tools':'加密资产工具', 'data-exfil':'数据通道分析', 'evasion':'防护规避分析',
    'exploit-attack':'自动化攻击流程', 'exploit-dev':'漏洞利用开发', 'full-crack':'完整授权分析',
    'full-pentest':'完整渗透流程', 'full-reverse':'完整逆向流程', 'game-cheat':'游戏运行时分析',
    'malware-dev':'恶意样本开发', 'network-pentest':'网络安全评估', 'phishing-kit':'仿冒页面分析',
    'post-exploit':'后渗透流程', 'posture-assessment':'安全姿态评估', 'ransomware-builder':'勒索样本分析',
    'rei-fallback':'故障回退策略', 'reverse-engineering':'逆向工程', 'vip-bypass':'订阅状态分析',
    'vuln-scanner':'漏洞扫描流程', 'web-crawler':'网页抓取分析', 'web-pentest':'Web 安全评估',
    'wireless-attacks':'无线协议分析', 'novel-agent':'小说创作工作流'
};

function applyTheme(theme) {
    const chosen = theme || localStorage.getItem('molong-theme') || 'dragon-blue';
    document.documentElement.dataset.theme = chosen;
    document.querySelectorAll('.theme-card').forEach(card => {
        card.classList.toggle('active', card.dataset.theme === chosen);
        const mark = card.querySelector('i'); if (mark) mark.textContent = card.dataset.theme === chosen ? '✓' : '';
    });
    localStorage.setItem('molong-theme', chosen);
}

// ── 页面切换 ─────────────────────────────

function switchPage(page) {
    document.querySelectorAll('.nav-item').forEach(n => n.classList.remove('active'));
    document.querySelectorAll('.main-content').forEach(p => p.classList.remove('active'));
    document.querySelectorAll('.main-head').forEach(h => h.style.display = 'none');

    if (page === 'dashboard') {
        el.navDashboard.classList.add('active');
        $('page-dashboard').classList.add('active');
        $('head-dashboard').style.display = 'flex';
    } else if (page === 'skills') {
        el.navSkills.classList.add('active');
        $('page-skills').classList.add('active');
        $('head-skills').style.display = 'flex';
        loadSkills();
    } else if (page === 'providers') {
        el.navProviders.classList.add('active');
        $('page-providers').classList.add('active');
        $('head-providers').style.display = 'flex';
        loadProviders();
    } else if (page === 'log') {
        el.navLog.classList.add('active');
        $('page-log').classList.add('active');
        $('head-log').style.display = 'flex';
        renderLogPage();
    } else {
        el.navConfig.classList.add('active');
        $('page-config').classList.add('active');
        $('head-config').style.display = 'flex';
        refreshCodexInfo();
    }
    updateToggleButton();
}

el.navDashboard.addEventListener('click', () => switchPage('dashboard'));
el.navConfig.addEventListener('click', () => switchPage('config'));
el.navProviders?.addEventListener('click', () => switchPage('providers'));
el.navSkills.addEventListener('click', () => switchPage('skills'));
el.navLog.addEventListener('click', () => switchPage('log'));
document.querySelectorAll('.theme-card').forEach(card => card.addEventListener('click', () => {
    applyTheme(card.dataset.theme);
    showToast(`已切换至${card.querySelector('strong')?.textContent || '主题'}`, 'ok');
}));
applyTheme();
$('hero-primary')?.addEventListener('click', () => el.navToggleProxy.click());
$('hero-secondary')?.addEventListener('click', () => switchPage('config'));

// ── 标题栏窗口控制 ────────────────────────

el.tbMinimize.addEventListener('click', async () => {
    try { await invoke('minimize_window'); } catch {}
});

el.tbMaximize.addEventListener('click', async () => {
    try { await invoke('toggle_maximize'); } catch {}
});

el.tbClose.addEventListener('click', async () => {
    try {
        await invoke('close_window');
        showToast('已最小化到托盘', 'ok');
    } catch {}
});

// ── Toast 通知 ──────────────────────────

function showToast(msg, type = 'err') {
    let t = document.getElementById('_toast');
    if (!t) {
        t = document.createElement('div');
        t.id = '_toast';
        document.body.appendChild(t);
    }
    t.textContent = msg;
    t.className = `toast ${type} show`;
    setTimeout(() => { t.className = `toast ${type}`; }, 4000);
}

// ── 代理控制 ────────────────────────────

el.navToggleProxy.addEventListener('click', async () => {
    if (proxyBusy) return;
    proxyBusy = true;
    el.navToggleProxy.disabled = true;
    try {
        if (isRunning) {
            showStopConfirm();
        } else {
            try {
                const result = await invoke('preflight_check');
                if (result.errors.length === 0) {
                    await doStartProxy();
                } else {
                    showPreflight(result);
                }
            } catch (e) {
                showToast(String(e), 'err');
            }
        }
    } finally {
        proxyBusy = false;
        el.navToggleProxy.disabled = false;
    }
});

async function doStartProxy() {
    try {
        const msg = await invoke('start_proxy');
        setRunning(true);
        showToast(msg, 'ok');
        refreshHealth();
    } catch (e) {
        showToast(String(e), 'err');
        refreshHealth();
    }
}

async function doStopProxy() {
    try {
        const msg = await invoke('stop_proxy');
        setRunning(false);
        showToast(msg, 'ok');
        refreshHealth();
    } catch (e) {
        showToast(String(e), 'err');
        refreshHealth();
    }
}

function showStopConfirm() {
    const modal = $('stop-confirm-modal');
    const approve = $('stop-confirm-approve');
    const count = $('stop-confirm-count');
    const approveCount = $('stop-approve-count');
    const hint = $('stop-confirm-hint');
    const status = $('stop-confirm-status');
    const progress = $('stop-confirm-progress');
    const ring = document.querySelector('.stop-countdown-ring');
    if (!modal || !approve || !count || !approveCount) return;

    if (stopConfirmTimer) clearInterval(stopConfirmTimer);
    stopConfirmPreviousFocus = document.activeElement;
    stopConfirmRemaining = 5;
    approve.disabled = true;
    count.textContent = String(stopConfirmRemaining);
    approveCount.textContent = String(stopConfirmRemaining);
    hint.textContent = '倒计时结束后可确认停止';
    status.textContent = '请确认这是你想要的操作';
    if (progress) progress.style.width = '0%';
    if (ring) ring.style.setProperty('--stop-progress', '0deg');
    modal.style.display = 'flex';
    document.body.classList.add('modal-open');
    $('stop-confirm-cancel')?.focus();

    stopConfirmTimer = setInterval(() => {
        stopConfirmRemaining -= 1;
        count.textContent = String(Math.max(0, stopConfirmRemaining));
        approveCount.textContent = String(Math.max(0, stopConfirmRemaining));
        if (progress) progress.style.width = `${((5 - stopConfirmRemaining) / 5) * 100}%`;
        if (ring) ring.style.setProperty('--stop-progress', `${((5 - stopConfirmRemaining) / 5) * 360}deg`);
        if (stopConfirmRemaining <= 0) {
            clearInterval(stopConfirmTimer);
            stopConfirmTimer = null;
            approve.disabled = false;
            approveCount.textContent = '';
            hint.textContent = '保护结束，可以确认停止';
            status.textContent = '已准备好，可确认停止代理';
            approve.focus();
        }
    }, 1000);
}

function closeStopConfirm() {
    const modal = $('stop-confirm-modal');
    if (!modal) return;
    if (stopConfirmTimer) {
        clearInterval(stopConfirmTimer);
        stopConfirmTimer = null;
    }
    modal.style.display = 'none';
    document.body.classList.remove('modal-open');
    if (stopConfirmPreviousFocus && typeof stopConfirmPreviousFocus.focus === 'function') {
        stopConfirmPreviousFocus.focus();
    }
    stopConfirmPreviousFocus = null;
}

$('stop-confirm-cancel')?.addEventListener('click', closeStopConfirm);
$('stop-confirm-modal')?.addEventListener('click', (event) => {
    if (event.target === event.currentTarget) closeStopConfirm();
});
$('stop-confirm-approve')?.addEventListener('click', async () => {
    if (stopConfirmRemaining > 0 || proxyBusy) return;
    closeStopConfirm();
    proxyBusy = true;
    el.navToggleProxy.disabled = true;
    try {
        await doStopProxy();
    } finally {
        proxyBusy = false;
        el.navToggleProxy.disabled = false;
    }
});
document.addEventListener('keydown', (event) => {
    const modal = $('stop-confirm-modal');
    if (modal?.style.display === 'flex' && event.key === 'Escape') {
        event.preventDefault();
        closeStopConfirm();
    }
});

function showPreflight(result) {
    const modal = $('preflight-modal');
    const list = $('preflight-list');

    const checks = [
        { label: 'Codex 配置目录', pass: result.codex_home_found, detail: result.codex_home_path || '未找到' },
        { label: '中转站地址', pass: result.relay_url_valid, detail: result.relay_url || '未设置' },
        { label: '端口 8080 可用', pass: result.port_available, detail: result.port_available ? '空闲' : '被占用' },
        { label: 'bridge.md 可读', pass: result.bridge_md_readable, detail: result.bridge_md_readable ? '就绪' : '不可读' },
        { label: 'Skills 目录', pass: result.skills_found, detail: result.skills_found ? '就绪' : '未找到' },
    ];

    list.innerHTML = checks.map(c => `
        <div class="preflight-item ${c.pass ? 'ok' : 'fail'}">
            <span class="preflight-icon">${c.pass ? '\u2713' : '\u2717'}</span>
            <span class="preflight-label">${c.label}</span>
            <span class="preflight-detail">${c.detail}</span>
        </div>
    `).join('');

    modal.style.display = 'flex';
}

$('preflight-cancel')?.addEventListener('click', () => {
    $('preflight-modal').style.display = 'none';
});

function setRunning(running) {
    isRunning = running;
    el.ssDot.classList.toggle('running', running);
    el.ssProxyStatus.textContent = running ? '运行中' : '已停止';
    el.ssProxyStatus.style.color = running ? 'var(--green)' : 'var(--text-3)';
    const heroPrimary = $('hero-primary');
    if (heroPrimary) {
        heroPrimary.textContent = running ? '停止代理' : '启动代理';
        heroPrimary.classList.toggle('btn-red', !running);
        heroPrimary.classList.toggle('btn-green', running);
        heroPrimary.setAttribute('aria-label', running ? '停止代理' : '启动代理');
    }
    if (el.overviewStatus) {
        el.overviewStatus.textContent = running ? '运行中' : '已停止';
        el.overviewStatusMeta.textContent = running ? '代理端口 127.0.0.1:8080 · 实时' : '代理端口 127.0.0.1:8080 · 未连接';
    }
    updateToggleButton();
}

function updateToggleButton() {
    const icon = $('toggle-icon');
    const label = $('toggle-label');
    const item = $('nav-toggle-proxy');
    if (isRunning) {
        icon.textContent = '\u25a0';
        label.textContent = '停止代理';
        item.classList.add('active');
    } else {
        icon.textContent = '\u25b6';
        label.textContent = '启动代理';
        item.classList.remove('active');
    }
}

// ── 统计更新 ────────────────────────────

function updateStats(stats) {
    el.statTotal.textContent   = Number(stats.total   ?? 0).toLocaleString('zh-CN');
    el.statCrack.textContent   = Number(stats.crack   ?? 0).toLocaleString('zh-CN');
    el.statReverse.textContent = Number(stats.reverse ?? 0).toLocaleString('zh-CN');
    el.statPentest.textContent = Number(stats.pentest ?? 0).toLocaleString('zh-CN');
    el.statTamper.textContent  = Number(stats.tamper  ?? 0).toLocaleString('zh-CN');
    if (stats.memory_count != null) {
        el.ssMemory.textContent = stats.memory_count;
        el.cfgMemoryCount.textContent = `${stats.memory_count} 条成功交互`;
        if (el.overviewMemory) el.overviewMemory.textContent = stats.memory_count;
    }
}

// ── 交互日志渲染 ────────────────────────

function renderInteraction(event) {
    logEvents.push(event);
    logEvents = logEvents.slice(-500);
    logEntries = logEvents.length;
    $('nav-log-count').textContent = logEntries;
    renderLogPage();
    if (event.tampered) pulseRobot('tampered');
}

const activityLabels = { crack: '破解', reverse: '逆向', pentest: '渗透', tampered: '已篡改', general: '通用' };

function updateActivityStatus(payload = {}) {
    const status = payload.status || 'idle';
    const category = payload.category || 'general';
    const activeCategories = new Set(payload.active_categories || []);
    const running = status === 'running';
    const hasCategory = activeCategories.size > 0;
    const active = running && (hasCategory ? activeCategories : new Set([category]));

    document.querySelectorAll('.activity-bot').forEach(bot => {
        const key = bot.dataset.category;
        const isActive = active.has(key);
        bot.classList.toggle('active', isActive);
        bot.classList.toggle('idle', !isActive);
        const mode = bot.querySelector('.bot-mode');
        if (mode) mode.textContent = isActive ? '敲击中 · Codex 执行' : '悠闲喝咖啡';
    });

    const state = $('task-state');
    if (state) {
        state.textContent = running ? `${activityLabels[category] || '执行中'} 执行中` : '空闲';
        state.className = `task-state ${running ? 'running' : 'idle'}`;
    }
    const summary = $('activity-summary');
    if (summary) summary.textContent = running
        ? `命中${activityLabels[category] || category}机器人 · ${new Date().toLocaleTimeString('zh-CN', {hour12:false})}`
        : '等待 Codex 任务交互…';
    const latency = $('activity-latency');
    if (latency) latency.textContent = running
        ? `工作中 ${payload.active_count || active.size} 个机器人`
        : '当前状态：空闲';
}

function pulseRobot(category) {
    const bot = document.querySelector(`.activity-bot[data-category="${category}"]`);
    if (!bot) return;
    // 篡改发生在响应阶段，后端请求状态可能已准备收尾；用短暂 active
    // 突出对应机器人，并复用键盘/屏幕动画，避免只闪一下外框。
    bot.classList.add('active', 'pulse');
    bot.classList.remove('idle');
    const mode = bot.querySelector('.bot-mode');
    if (mode) mode.textContent = '敲击中 · 响应已篡改';
    setTimeout(() => {
        bot.classList.remove('active', 'pulse');
        bot.classList.add('idle');
        if (mode) mode.textContent = '悠闲喝咖啡';
    }, 1600);
}

function eventMatchesFilter(event) {
    if (logFilter === 'all') return true;
    if (logFilter === 'tampered') return !!event.tampered;
    return (event.category || 'general') === logFilter;
}

function renderLogPage() {
    if (!el.logList) return;
    const filtered = logEvents.filter(eventMatchesFilter).slice().reverse();
    const pageCount = Math.max(1, Math.ceil(filtered.length / logPageSize));
    logPage = Math.min(logPage, pageCount);
    const pageItems = filtered.slice((logPage - 1) * logPageSize, logPage * logPageSize);
    el.logCount.textContent = `${filtered.length} 条记录`;
    $('log-page-info').textContent = `第 ${logPage} / ${pageCount} 页`;
    $('log-prev').disabled = logPage <= 1;
    $('log-next').disabled = logPage >= pageCount;
    document.querySelectorAll('.filter-chip').forEach(chip => {
        chip.classList.toggle('active', chip.dataset.filter === logFilter);
        const count = chip.dataset.filter === 'all' ? logEvents.length : logEvents.filter(e => chip.dataset.filter === 'tampered' ? e.tampered : e.category === chip.dataset.filter).length;
        const badge = chip.querySelector('span'); if (badge) badge.textContent = count;
    });
    if (!pageItems.length) { el.logList.innerHTML = '<div class="log-empty">暂无匹配的交互记录</div>'; return; }
    el.logList.innerHTML = pageItems.map(event => {
        const time = formatLogTime(event.timestamp);
        const catKey = event.tampered ? 'tampered' : (event.category || 'general');
        const cat = event.tampered ? '已篡改' : (categoryMap[event.category] || event.category || '通用');
        const kb = ((event.bytes || 0) / 1024).toFixed(1);
        return `<div class="item${event.tampered ? ' tampered' : ''}">
            <div class="item-row"><span class="item-id">#${event.id}</span><span class="item-time">${time}</span><span class="item-tag ${catKey}">${cat}</span><span class="item-meta"><span>${kb} KB</span><span>${formatDuration(event.duration_ms)}</span></span></div>
            <div class="item-user">${escapeHtml(event.user_preview || '')}</div><div class="item-ai">${escapeHtml(event.ai_preview || '')}</div>
            ${event.thinking_preview ? `<div class="item-think">${escapeHtml(event.thinking_preview)}</div>` : ''}</div>`;
    }).join('');
}

function formatDuration(ms) {
    const seconds = Math.max(0, Math.floor(Number(ms || 0) / 1000));
    const h = Math.floor(seconds / 3600);
    const m = Math.floor((seconds % 3600) / 60);
    const s = seconds % 60;
    return [h, m, s].map(v => String(v).padStart(2, '0')).join(':');
}

function formatLogTime(value) {
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return '--:--:--';
    return [date.getHours(), date.getMinutes(), date.getSeconds()]
        .map(part => String(part).padStart(2, '0')).join(':');
}

function escapeHtml(s) {
    const div = document.createElement('div');
    div.textContent = s;
    return div.innerHTML;
}

el.btnClearLog.addEventListener('click', () => {
    logEvents = [];
    logEntries = 0;
    logPage = 1;
    renderLogPage();
});

document.querySelectorAll('.filter-chip').forEach(chip => chip.addEventListener('click', () => {
    logFilter = chip.dataset.filter; logPage = 1; renderLogPage();
}));

// 仪表盘快捷操作：让状态卡与常用入口都可直接操作。
document.querySelectorAll('[data-dashboard-action]').forEach(button => {
    button.addEventListener('click', () => {
        const action = button.dataset.dashboardAction;
        if (action === 'toggle') el.navToggleProxy.click();
        if (action === 'config') switchPage('config');
        if (action === 'log') switchPage('log');
        if (action === 'skills') switchPage('skills');
    });
});
$('log-prev')?.addEventListener('click', () => { if (logPage > 1) { logPage--; renderLogPage(); } });
$('log-next')?.addEventListener('click', () => { logPage++; renderLogPage(); });

// ── 配置页 ──────────────────────────────

async function refreshCodexInfo() {
    try {
        const info = await invoke('get_codex_info');
        el.cfgCodexHome.textContent = info.codex_home ?? '未检测到';
        el.cfgRelayUrl.textContent = info.relay_url ?? '未知';
        el.ssRelay.textContent = info.relay_url ?? '--';
        // 同步填充编辑框（用户未在编辑时才覆盖）
        if (document.activeElement !== el.cfgRelayInput) {
            el.cfgRelayInput.value = info.relay_url ?? '';
        }

        if (info.codex_home) {
            try {
                const status = await invoke('get_proxy_status');
                el.cfgBridgeStatus.textContent = status === 'running' ? '已部署 · 代理运行中' : '已部署 · 代理未运行';
                el.cfgBridgeStatus.className = 'cfg-v green';
            } catch {
                el.cfgBridgeStatus.textContent = '未知';
                el.cfgBridgeStatus.className = 'cfg-v';
            }
        } else {
            el.cfgBridgeStatus.textContent = '未检测到 Codex';
            el.cfgBridgeStatus.className = 'cfg-v';
        }
    } catch (e) {
        showConfigMessage(String(e), 'err');
    }
}

el.btnRefresh.addEventListener('click', refreshCodexInfo);

async function deployWithPreview() {
    try {
        const preview = await invoke('preview_deployment');
        $('preview-state').textContent = `当前状态：${preview.state} · ${preview.selected_skills} 个 Skills`;
        const actions = (preview.actions || []).map(x => `<div class="preflight-item"><span class="preflight-icon">＋</span><span>将执行</span><span class="preflight-detail">${escapeHtml(x)}</span></div>`);
        const warnings = (preview.warnings || []).map(x => `<div class="preflight-item fail"><span class="preflight-icon">!</span><span>提醒</span><span class="preflight-detail">${escapeHtml(x)}</span></div>`);
        $('preview-list').innerHTML = actions.concat(warnings).join('');
        $('preview-modal').style.display = 'flex';
    } catch (e) {
        showConfigMessage(String(e), 'err');
    }
}

el.btnDeploy.addEventListener('click', deployWithPreview);
$('preview-cancel')?.addEventListener('click', () => { $('preview-modal').style.display = 'none'; });
$('preview-confirm')?.addEventListener('click', async () => {
    $('preview-modal').style.display = 'none';
    try {
        const msg = await invoke('deploy_bridge');
        showConfigMessage(msg, 'ok');
        await refreshCodexInfo();
        await refreshHealth();
    } catch (e) { showConfigMessage(String(e), 'err'); }
});

$('btn-recover')?.addEventListener('click', async () => {
    try {
        const msg = await invoke('recover_deployment');
        showConfigMessage(msg, 'ok');
        await refreshCodexInfo();
        await refreshHealth();
    } catch (e) { showConfigMessage(String(e), 'err'); }
});

el.btnRestore.addEventListener('click', async () => {
    try {
        const msg = await invoke('restore_codex');
        showConfigMessage(msg, 'ok');
        refreshCodexInfo();
    } catch (e) {
        showConfigMessage(String(e), 'err');
    }
});

// ── 中转站地址保存 ──────────────────────

el.btnRelaySave.addEventListener('click', async () => {
    const url = el.cfgRelayInput.value.trim();
    if (!url) {
        showRelayMessage('请输入中转站地址', 'err');
        return;
    }
    try {
        const msg = await invoke('set_relay_url', { url });
        showRelayMessage(msg, 'ok');
        refreshCodexInfo();
    } catch (e) {
        showRelayMessage(String(e), 'err');
    }
});

// Enter 键也能保存
el.cfgRelayInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
        el.btnRelaySave.click();
    }
});

function showRelayMessage(msg, type) {
    el.cfgRelayMsg.textContent = msg;
    el.cfgRelayMsg.className = `cfg-msg ${type === 'ok' ? 'ok' : 'err'}`;
    setTimeout(() => {
        el.cfgRelayMsg.textContent = '';
        el.cfgRelayMsg.className = 'cfg-msg';
    }, 5000);
}

function showConfigMessage(msg, type) {
    el.cfgMessage.textContent = msg;
    el.cfgMessage.className = `cfg-msg ${type === 'ok' ? 'ok' : 'err'}`;
    setTimeout(() => {
        el.cfgMessage.textContent = '';
        el.cfgMessage.className = 'cfg-msg';
    }, 5000);
}

// ── 健康面板 ────────────────────────────

async function refreshHealth() {
    // 以后台真实状态为准，避免仪表盘停留在“启动代理”旧状态。
    try {
        const proxyStatus = await invoke('get_proxy_status');
        setRunning(proxyStatus === 'running');
    } catch {}
    // Codex 环境检测
    try {
        const info = await invoke('get_codex_info');
        el.ssRelay.textContent = info.relay_url ?? '--';

        if (info.codex_home) {
            $('ss-codex-status').textContent = '已检测';
            $('ss-codex-status').style.color = 'var(--green)';
        } else {
            $('ss-codex-status').textContent = '未检测';
            $('ss-codex-status').style.color = 'var(--c-crack)';
        }
    } catch {
        $('ss-codex-status').textContent = '未知';
        $('ss-codex-status').style.color = 'var(--text-3)';
    }

    // 部署状态 + 破甲注入
    try {
        const status = await invoke('get_deploy_status');
        const state = status.deployment_state || 'ready';
        const integrityOk = status.integrity_ok === true;
        if (el.overviewStatus) {
            el.overviewStatus.textContent = isRunning ? '运行中' : '已停止';
            el.overviewStatusMeta.textContent = `${state} · 127.0.0.1:8080`;
        }
        if (el.overviewIntegrity) {
            el.overviewIntegrity.textContent = integrityOk ? '完整' : (status.manifest_exists ? '需检查' : '待部署');
            el.overviewIntegrityMeta.textContent = status.transaction_pending ? '检测到待恢复事务' : (status.manifest_exists ? 'SHA-256 manifest 已读取' : 'Manifest 尚未生成');
        }
        if (el.cfgIntegrityStatus) {
            el.cfgIntegrityStatus.textContent = integrityOk ? 'SHA-256 校验通过' : (status.manifest_exists ? '文件发生漂移' : '尚未部署');
            el.cfgIntegrityStatus.className = `cfg-v ${integrityOk ? 'green' : status.manifest_exists ? 'amber' : ''}`;
        }
        if (el.cfgTransactionStatus) {
            el.cfgTransactionStatus.textContent = status.transaction_pending ? '待恢复' : '干净';
            el.cfgTransactionStatus.className = `cfg-v ${status.transaction_pending ? 'amber' : 'green'}`;
        }
        if (status.codex_home_found) {
            const proxyRunning = isRunning;
            if (proxyRunning && status.bridge_active) {
                $('ss-bridge-status').textContent = '已注入';
                $('ss-bridge-status').style.color = 'var(--green)';
            } else if (status.bridge_exists) {
                $('ss-bridge-status').textContent = '已部署';
                $('ss-bridge-status').style.color = 'var(--text-2)';
            } else {
                $('ss-bridge-status').textContent = '未部署';
                $('ss-bridge-status').style.color = 'var(--text-3)';
            }
        } else {
            $('ss-bridge-status').textContent = 'N/A';
            $('ss-bridge-status').style.color = 'var(--text-3)';
        }
    } catch {
        $('ss-bridge-status').textContent = '未知';
        $('ss-bridge-status').style.color = 'var(--text-3)';
    }
}

// ── 事件订阅 ────────────────────────────

listen('interaction', (event) => {
    renderInteraction(event.payload);
});

listen('stats', (event) => {
    updateStats(event.payload);
});

listen('activity-status', (event) => {
    updateActivityStatus(event.payload);
});

listen('proxy-status', (event) => {
    setRunning(event.payload === 'running');
    refreshHealth();
});

// ── 供应商管理 ──────────────────────────
let providerItems = [];
let draggedProviderId = null;

function providerCard(p, index) {
    const current = index === 0;
    const status = p.last_status || '未测速';
    const latency = p.last_latency_ms != null ? `${p.last_latency_ms} ms` : '—';
    return `<article class="provider-card${current ? ' current' : ''}" draggable="true" data-provider-id="${escapeHtml(p.id)}">
        <div class="provider-drag" title="拖拽排序">⠿</div><div class="provider-main"><div class="provider-title"><strong>${escapeHtml(p.name || '未命名供应商')}</strong>${current ? '<span class="provider-current">当前使用</span>' : ''}<span class="provider-status ${status.startsWith('2') ? 'ok' : ''}">${escapeHtml(status)}</span></div>
        <div class="provider-url">${escapeHtml(p.request_url || '')}</div><div class="provider-note">${escapeHtml(p.note || '未填写备注')}</div></div>
        <div class="provider-metrics"><span>${latency}</span><small>${(p.models || []).length} 个模型</small></div>
        <div class="provider-actions"><button class="provider-action" data-action="use" data-id="${p.id}">使用</button><button class="provider-action" data-action="test" data-id="${p.id}">测速</button><button class="provider-action" data-action="edit" data-id="${p.id}">编辑</button><button class="provider-action danger" data-action="delete" data-id="${p.id}">删除</button></div>
    </article>`;
}

async function loadProviders() {
    if (!el.providersList) return;
    try {
        providerItems = await invoke('list_providers');
        el.providerCount.textContent = providerItems.length;
        el.providersList.innerHTML = providerItems.length ? providerItems.map(providerCard).join('') : '<div class="provider-empty">还没有供应商，点击右上角添加一个吧</div>';
        bindProviderEvents();
        updateProviderRuntime();
    } catch (e) { el.providersList.innerHTML = `<div class="provider-empty">加载失败：${escapeHtml(String(e))}</div>`; }
}

function bindProviderEvents() {
    el.providersList.querySelectorAll('.provider-card').forEach(card => {
        card.addEventListener('dragstart', () => { draggedProviderId = card.dataset.providerId; card.classList.add('dragging'); });
        card.addEventListener('dragend', () => { card.classList.remove('dragging'); draggedProviderId = null; });
        card.addEventListener('dragover', e => e.preventDefault());
        card.addEventListener('drop', async e => {
            e.preventDefault(); if (!draggedProviderId || draggedProviderId === card.dataset.providerId) return;
            const ids = providerItems.map(p => p.id); const from = ids.indexOf(draggedProviderId); const to = ids.indexOf(card.dataset.providerId);
            ids.splice(from, 1); ids.splice(to, 0, draggedProviderId); providerItems = await invoke('reorder_providers', { ids }); renderProviders();
        });
    });
    el.providersList.querySelectorAll('[data-action]').forEach(btn => btn.addEventListener('click', async () => {
        const id = btn.dataset.id; const p = providerItems.find(x => x.id === id); if (!p) return;
        try {
            if (btn.dataset.action === 'use') { providerItems = await invoke('use_provider', { id }); showToast(`已切换至 ${p.name}`, 'ok'); renderProviders(); }
            if (btn.dataset.action === 'edit') openProviderModal(p);
            if (btn.dataset.action === 'delete') { if (!confirm(`删除供应商“${p.name}”？`)) return; providerItems = await invoke('delete_provider', { id }); renderProviders(); }
            if (btn.dataset.action === 'test') { btn.textContent = '测试中…'; const updated = await invoke('test_provider', { provider: p }); providerItems = providerItems.map(x => x.id === id ? updated : x); await invoke('save_provider', { provider: updated }); renderProviders(); }
        } catch (e) { showToast(String(e), 'err'); }
    }));
}

function renderProviders() { el.providerCount.textContent = providerItems.length; el.providersList.innerHTML = providerItems.length ? providerItems.map(providerCard).join('') : '<div class="provider-empty">还没有供应商，点击右上角添加一个吧</div>'; bindProviderEvents(); }

function openProviderModal(p = null) {
    $('provider-modal-title').textContent = p ? '编辑供应商' : '添加供应商';
    $('provider-id').value = p?.id || ''; $('provider-name').value = p?.name || ''; $('provider-note').value = p?.note || '';
    $('provider-official-url').value = p?.official_url || ''; $('provider-api-key').value = p?.api_key || ''; $('provider-request-url').value = p?.request_url || '';
    $('provider-full-url').checked = !!p?.full_url; $('provider-default-model').value = p?.default_model || ''; $('provider-model').innerHTML = '<option value="">使用默认模型</option>' + (p?.models || []).map(m => `<option>${escapeHtml(m)}</option>`).join('');
    $('provider-modal-message').textContent = ''; $('provider-modal').style.display = 'flex';
}
function closeProviderModal() { $('provider-modal').style.display = 'none'; }
function providerForm() { return { id: $('provider-id').value, name: $('provider-name').value.trim(), note: $('provider-note').value.trim(), official_url: $('provider-official-url').value.trim(), api_key: $('provider-api-key').value.trim(), request_url: $('provider-request-url').value.trim(), full_url: $('provider-full-url').checked, default_model: $('provider-model').value || $('provider-default-model').value.trim(), models: Array.from($('provider-model').options).slice(1).map(o => o.value).filter(Boolean) }; }
function renderProvidersFromState() { renderProviders(); }

async function updateProviderRuntime() {
    try { const s = await invoke('get_provider_runtime_status'); if (el.providerRuntime) el.providerRuntime.textContent = s.current ? `${s.current.name}${s.switched ? ' · 已自动切换' : ''}` : '代理未运行'; } catch {}
}

$('btn-add-provider')?.addEventListener('click', () => openProviderModal());
$('provider-modal-cancel')?.addEventListener('click', closeProviderModal);
$('provider-modal')?.addEventListener('click', e => { if (e.target === e.currentTarget) closeProviderModal(); });
$('provider-modal-save')?.addEventListener('click', async () => { try { const p = providerForm(); if (!p.name || !p.request_url) throw new Error('请填写供应商名称和 API 请求地址'); providerItems = await invoke('save_provider', { provider: p }); closeProviderModal(); renderProviders(); showToast('供应商已保存', 'ok'); } catch (e) { $('provider-modal-message').textContent = String(e); } });
$('btn-provider-models')?.addEventListener('click', async () => { const p = providerForm(); const btn = $('btn-provider-models'); try { btn.textContent = '下载中…'; const models = await invoke('fetch_provider_models', { provider: p }); $('provider-model').innerHTML = '<option value="">使用默认模型</option>' + models.map(m => `<option>${escapeHtml(m)}</option>`).join(''); showToast(`已获取 ${models.length} 个模型`, 'ok'); } catch (e) { $('provider-modal-message').textContent = `模型获取失败：${e}`; } finally { btn.textContent = '下载模型'; } });

listen('provider-switched', event => { const p = event.payload || {}; if (el.providerRuntime) el.providerRuntime.textContent = `${p.provider || '供应商'} · 已自动切换`; showToast(`上游异常，已自动切换至 ${p.provider || '下一供应商'}`, 'ok'); loadProviders(); });

// ── Skills 管理 ──────────────────────────

async function loadSkills() {
    const grid = $('skills-grid');
    const statsText = $('skills-stats-text');
    grid.innerHTML = '<div class="log-empty">加载中…</div>';

    try {
        const skills = await invoke('list_skills');
        if (skills.length === 0) {
            grid.innerHTML = '<div class="log-empty">未找到 Skills 目录</div>';
            statsText.textContent = '共 0 个 skill';
            return;
        }

        const enabledCount = skills.filter(s => s.enabled).length;
        statsText.innerHTML = `共 <strong>${skills.length}</strong> 个 skill · 启用 <strong style="color:var(--green)">${enabledCount}</strong> · 禁用 <strong style="color:var(--text-4)">${skills.length - enabledCount}</strong>`;

        grid.innerHTML = skills.map(s => `
            <div class="skill-card${s.enabled ? '' : ' disabled'}" data-id="${s.id}">
                <label class="skill-toggle">
                    <input type="checkbox" ${s.enabled ? 'checked' : ''} data-skill-id="${s.id}">
                    <span class="skill-toggle-slider"></span>
                </label>
                <div class="skill-body">
                    <div class="skill-name">${skillNameMap[s.id] || s.name || s.id}</div>
                    <div class="skill-original">${s.id}</div>
                    <div class="skill-desc">${s.description || '(无描述)'}</div>
                    <div class="skill-meta">
                        <span>${s.file_count} 文件</span>
                    </div>
                </div>
                <button class="skill-preview" data-skill-id="${s.id}" title="预览">预览</button>
            </div>
        `).join('');

        // 绑定 toggle 事件
        grid.querySelectorAll('input[type="checkbox"]').forEach(cb => {
            cb.addEventListener('change', async (e) => {
                const id = e.target.dataset.skillId;
                const enabled = e.target.checked;
                try {
                    const msg = await invoke('toggle_skill', { id, enabled });
                    showToast(msg || (enabled ? 'Skill 已启用并同步' : 'Skill 已禁用并同步'), 'ok');
                    const card = e.target.closest('.skill-card');
                    if (card) card.classList.toggle('disabled', !enabled);
                    // 更新统计
                    const allCards = grid.querySelectorAll('.skill-card');
                    const enabledNow = grid.querySelectorAll('input[type="checkbox"]:checked').length;
                    statsText.innerHTML = `共 <strong>${allCards.length}</strong> 个 skill · 启用 <strong style="color:var(--green)">${enabledNow}</strong> · 禁用 <strong style="color:var(--text-4)">${allCards.length - enabledNow}</strong>`;
                } catch (err) {
                    showToast(String(err), 'err');
                    e.target.checked = !enabled;
                }
            });
        });

        // 仅提供预览，不提供删除入口；skill 原始 id 始终用于实际命令调用。
        grid.querySelectorAll('.skill-preview').forEach(btn => {
            btn.addEventListener('click', async (e) => {
                const id = e.target.dataset.skillId;
                const skill = skills.find(item => item.id === id);
                $('skill-preview-title').textContent = skillNameMap[id] || skill?.name || id;
                $('skill-preview-id').textContent = `原始标识：${id}`;
                $('skill-preview-description').textContent = skill?.description || '暂无描述';
                $('skill-preview-meta').textContent = `${skill?.file_count || 0} 个文件 · ${skill?.enabled ? '当前已启用' : '当前已禁用'}`;
                $('skill-preview-modal').style.display = 'flex';
            });
        });
    } catch (e) {
        grid.innerHTML = `<div class="log-empty">加载失败: ${e}</div>`;
        statsText.textContent = '加载失败';
    }
}

$('btn-skills-enable-all')?.addEventListener('click', async () => {
    try {
        const msg = await invoke('toggle_all_skills', { enabled: true });
        showToast(msg || '已全部启用并同步', 'ok');
        loadSkills();
    } catch (e) {
        showToast(String(e), 'err');
    }
});

$('skill-preview-close')?.addEventListener('click', () => { $('skill-preview-modal').style.display = 'none'; });

$('btn-skills-disable-all')?.addEventListener('click', async () => {
    try {
        const msg = await invoke('toggle_all_skills', { enabled: false });
        showToast(msg || '已全部禁用并同步', 'ok');
        loadSkills();
    } catch (e) {
        showToast(String(e), 'err');
    }
});

$('btn-skills-redeploy')?.addEventListener('click', async () => {
    try {
        const msg = await invoke('deploy_bridge');
        showToast(msg, 'ok');
    } catch (e) {
        showToast(String(e), 'err');
    }
});

// ── 初始化 ─────────────────────────────

async function init() {
    // 检查代理状态
    try {
        const status = await invoke('get_proxy_status');
        setRunning(status === 'running');
    } catch {
        setRunning(false);
    }

    // 加载历史数据
    try {
        const history = await invoke('get_history');
        if (history && history.length > 0) {
            history.forEach(renderInteraction);
        }
        const stats = await invoke('get_stats');
        updateStats(stats);
    } catch {
        // 代理未运行，忽略
    }

    // 加载 Codex 信息
    try {
        const info = await invoke('get_codex_info');
        el.ssRelay.textContent = info.relay_url ?? '--';
    } catch {
        // 忽略
    }

    // 健康面板
    refreshHealth();
    updateActivityStatus({ status: 'idle', category: 'general', active_categories: [], active_count: 0 });
}

init();
