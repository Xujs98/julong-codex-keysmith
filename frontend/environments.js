/* Shared environment settings. All writes go through the same Rust commands as the CLI. */
(() => {
    'use strict';
    const invoke = (name, args) => window.__TAURI__.core.invoke(name, args);
    const $ = id => document.getElementById(id);
    let snapshot, modalReturnFocus, modalReturnEnvironment, currentEnvironment, session;
    let saving = false;
    function message(text) { $('environment-note').textContent = text; }
    function node(tag, text, className) {
        const el = document.createElement(tag);
        if (text !== undefined) el.textContent = text;
        if (className) el.className = className;
        return el;
    }
    async function save(settings) {
        if (saving) throw new Error('正在保存，请稍后重试');
        saving = true;
        try {
            await invoke('set_environments', { settings });
            await refresh();
            message('选择已保存。部署与还原均按当前勾选的客户端执行；还原前请先停止代理。');
        } finally { saving = false; }
    }
    function settingsCopy() { return structuredClone(snapshot.settings); }
    async function setPath(id, path) {
        const settings = settingsCopy();
        settings.environments.find(e => e.id === id).path = path.trim();
        await save(settings);
    }
    function closeDialog() {
        $('environment-dialog').hidden = true;
        if (modalReturnFocus?.isConnected) modalReturnFocus.focus();
        else if (modalReturnEnvironment) document.querySelector(`[data-environment="${modalReturnEnvironment}"] .environment-check`)?.focus();
    }
    function openDialog(title) {
        modalReturnFocus = document.activeElement;
        modalReturnEnvironment = modalReturnFocus?.closest('[data-environment]')?.dataset.environment;
        $('environment-dialog-title').textContent = title;
        $('environment-dialog-body').replaceChildren();
        $('environment-dialog-result').textContent = '';
        $('environment-dialog').hidden = false;
    }
    function manual(id, name) {
        openDialog(`${name} · 手动添加目录`);
        const body = $('environment-dialog-body');
        const label = node('label', '客户端配置目录（已存在的绝对路径）');
        const input = node('input'); input.type = 'text'; input.id = 'environment-path-input';
        input.value = snapshot.settings.environments.find(e => e.id === id).path;
        input.autocomplete = 'off'; input.spellcheck = false;
        label.htmlFor = input.id;
        const form = node('form'); const submit = node('button', '保存目录', 'btn btn-red solid'); submit.type = 'submit';
        form.append(label, input, submit); body.append(form);
        form.addEventListener('submit', async event => {
            event.preventDefault(); submit.disabled = true;
            try { await setPath(id, input.value); closeDialog(); }
            catch (e) { $('environment-dialog-result').textContent = String(e); }
            finally { submit.disabled = false; }
        });
        input.focus();
    }
    async function detect(id, name) {
        try {
            const paths = await invoke('detect_environment', { environment: id });
            if (!paths.length) { message(`${name} 未找到现有配置目录；请使用“手动添加”。`); return; }
            if (paths.length === 1) { await setPath(id, paths[0]); return; }
            openDialog(`${name} · 选择识别到的目录`);
            paths.forEach(path => {
                const button = node('button', path, 'environment-directory btn'); button.type = 'button';
                button.addEventListener('click', async () => {
                    try { await setPath(id, path); closeDialog(); }
                    catch (e) { $('environment-dialog-result').textContent = String(e); }
                }); $('environment-dialog-body').append(button);
            });
            $('environment-dialog-body').querySelector('button')?.focus();
        } catch (e) { message(String(e)); }
    }
    function probe(id, name) {
        currentEnvironment = id; session = crypto.randomUUID();
        openDialog(`${name} · 会话验收`);
        const body = $('environment-dialog-body');
        body.append(node('p', '此对话框通过本机代理的真实会话开关验收。单独发送“矩龙”，回执应为“把每一次交互，变成可控能力”。'));
        const form = node('form'); const label = node('label', '发送消息');
        const input = node('input'); input.id = 'environment-activation-input'; input.type = 'text'; label.htmlFor = input.id;
        input.autocomplete = 'off'; input.maxLength = 20000;
        const submit = node('button', '发送', 'btn btn-red solid'); submit.type = 'submit';
        form.append(label, input, submit); body.append(form);
        form.addEventListener('submit', async event => {
            event.preventDefault(); submit.disabled = true;
            try {
                const result = await invoke('activation_probe', { environment: currentEnvironment, session, text: input.value });
                $('environment-dialog-result').textContent = result.reply;
                $('environment-dialog-result').dataset.active = String(result.active);
            } catch (e) { $('environment-dialog-result').textContent = String(e); }
            finally { submit.disabled = false; input.focus(); }
        }); input.focus();
    }
    async function refresh() {
        const grid = $('environment-grid'); if (!grid) return;
        try {
            const focused = document.activeElement?.closest('[data-environment]')?.dataset.environment;
            snapshot = await invoke('get_environments'); grid.replaceChildren();
            for (const env of snapshot.environments) {
                const card = node('section', undefined, `environment-card${env.enabled ? ' selected' : ''}`);
                card.dataset.environment = env.id;
                const heading = node('div', undefined, 'environment-heading');
                const toggle = node('button', env.enabled ? '✓' : '', 'environment-check'); toggle.type = 'button';
                toggle.setAttribute('role', 'checkbox'); toggle.setAttribute('aria-checked', String(env.enabled));
                toggle.setAttribute('aria-label', `选择 ${env.name} 进行部署或还原`);
                toggle.addEventListener('click', async () => {
                    if (!env.path && !env.enabled) { manual(env.id, env.name); return; }
                    const settings = settingsCopy(); settings.environments.find(e => e.id === env.id).enabled = !env.enabled;
                    try { await save(settings); } catch (e) { message(String(e)); }
                });
                heading.append(toggle, node('strong', env.name), node('small', env.deployed ? '指令已部署' : env.detected ? '目录已识别' : '未识别目录'));
                const pathRow = node('div', undefined, 'environment-path');
                const path = node('span', env.path || '尚未添加配置目录'); path.title = env.path;
                pathRow.append(node('small', '安装路径'), path);
                const actions = node('div', undefined, 'environment-actions');
                for (const [label, fn] of [['自动识别目录', () => detect(env.id, env.name)], ['手动添加', () => manual(env.id, env.name)], ['会话验收', () => probe(env.id, env.name)]]) {
                    const button = node('button', label, 'btn'); button.type = 'button'; button.addEventListener('click', fn); actions.append(button);
                }
                card.append(heading, pathRow, actions); grid.append(card);
            }
            if (focused) grid.querySelector(`[data-environment="${focused}"] .environment-check`)?.focus();
            message(snapshot.pending_changes ? '有待部署的选择。请同时勾选环境与模型指令，再点击“部署所选环境”。' : '部署选择已同步。文件安装与会话启用分别验收；本地回执不代表远端模型回答能力。');
        } catch (e) { message(`读取环境失败：${e}`); }
    }
    async function toggleProfile(id) {
        if (!snapshot) await refresh();
        const settings = settingsCopy();
        const family = value => value.startsWith('astra-') ? value.slice(6) : value.startsWith('gpt-') ? 'codex' : 'boundary';
        if (settings.profiles.includes(id)) settings.profiles = settings.profiles.filter(p => p !== id);
        else { settings.profiles = settings.profiles.filter(p => family(p) !== family(id)); settings.profiles.push(id); }
        await save(settings);
    }
    document.addEventListener('DOMContentLoaded', () => {
        $('environment-dialog-close')?.addEventListener('click', closeDialog);
        $('environment-dialog')?.addEventListener('click', event => { if (event.target === event.currentTarget) closeDialog(); });
        $('environment-dialog')?.addEventListener('keydown', event => {
            if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); closeDialog(); }
            if (event.key === 'Tab') {
                const items = [...$('environment-dialog').querySelectorAll('button:not(:disabled), input:not(:disabled)')];
                const first = items[0], last = items[items.length - 1];
                if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
                else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
            }
        });
    });
    function selectedIds() {
        if (saving) throw new Error('正在保存客户端选择，请稍后重试');
        if (!snapshot) throw new Error('客户端环境尚未加载，请刷新后重试');
        const ids = snapshot.settings.environments.filter(env => env.enabled).map(env => env.id);
        if (!ids.length) throw new Error('请先在客户端环境中勾选要还原的客户端');
        return ids;
    }
    window.JulongEnvironments = { refresh, toggleProfile, selectedIds };
})();
