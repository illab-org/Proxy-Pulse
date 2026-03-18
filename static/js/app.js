/* ─────────────────────────────────────────────
   Proxy Pulse — Dashboard Application Logic
   ───────────────────────────────────────────── */

const API_BASE = '/api/v1';
const REFRESH_INTERVAL = 30000; // 30 seconds

// Auth-aware fetch wrapper
function authFetch(url, options = {}) {
    const token = localStorage.getItem('pp_token');
    if (token) {
        options.headers = { ...options.headers, 'Authorization': `Bearer ${token}` };
    }
    return fetch(url, options).then(resp => {
        if (resp.status === 401) {
            localStorage.removeItem('pp_token');
            document.cookie = 'pp_token=; path=/; max-age=0';
            window.location.href = '/login';
        }
        return resp;
    });
}

function logout() {
    const token = localStorage.getItem('pp_token');
    if (token) {
        fetch('/api/v1/auth/logout', {
            method: 'POST',
            headers: { 'Authorization': `Bearer ${token}` },
        }).catch(() => {});
    }
    localStorage.removeItem('pp_token');
    document.cookie = 'pp_token=; path=/; max-age=0';
    window.location.href = '/login';
}

// Chart instances
let latencyChart = null;
let protocolChart = null;
let scoreChart = null;
let topProxyGroup = 'all';

// ─── Chart.js Global Defaults ───
function updateChartTheme() {
    const isLight = document.documentElement.getAttribute('data-theme') === 'light';
    Chart.defaults.color = isLight ? '#64748b' : '#8892b0';
}
updateChartTheme();
// Listen for theme changes
new MutationObserver(() => {
    updateChartTheme();
    if (latencyChart) latencyChart.update();
    if (protocolChart) protocolChart.update();
    if (scoreChart) scoreChart.update();
}).observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });

Chart.defaults.font.family = "'Inter', sans-serif";
Chart.defaults.font.size = 11;
Chart.defaults.plugins.legend.labels.usePointStyle = true;
Chart.defaults.plugins.legend.labels.pointStyleWidth = 8;
Chart.defaults.plugins.legend.labels.padding = 14;

// Palette
const COLORS = {
    cyan: '#00f0ff',
    purple: '#7b61ff',
    green: '#00e68a',
    red: '#ff4757',
    orange: '#ffa502',
    blue: '#3b82f6',
    pink: '#f472b6',
    teal: '#2dd4bf',
    yellow: '#facc15',
    indigo: '#818cf8',
};

const CHART_PALETTE = [
    '#00f0ff', '#7b61ff', '#00e68a', '#ffa502', '#3b82f6',
    '#f472b6', '#2dd4bf', '#facc15', '#818cf8', '#ff4757',
    '#a78bfa', '#34d399', '#60a5fa', '#fbbf24', '#fb923c',
    '#e879f9', '#38bdf8', '#4ade80', '#f87171', '#c084fc',
];

// ─── Initialize ───
document.addEventListener('DOMContentLoaded', async () => {
    await I18N.ready;
    initCharts();
    initTopGroupFilter();
    await loadTopProxyGroups();
    fetchAllData();
    setInterval(fetchAllData, REFRESH_INTERVAL);

    document.getElementById('refreshBtn').addEventListener('click', () => {
        const btn = document.getElementById('refreshBtn');
        btn.classList.add('spinning');
        fetchAllData().then(() => {
            setTimeout(() => btn.classList.remove('spinning'), 500);
        });
    });
});

// ─── Fetch All Data ───
async function fetchAllData() {
    try {
        let topUrl = `${API_BASE}/proxy/top?limit=20`;
        if (topProxyGroup && topProxyGroup !== 'all') {
            topUrl += `&group=${encodeURIComponent(topProxyGroup)}`;
        }

        const [statsRes, topRes] = await Promise.all([
            authFetch(`${API_BASE}/proxy/stats`),
            authFetch(topUrl),
        ]);

        if (statsRes.ok) {
            const statsData = await statsRes.json();
            if (statsData.success) {
                updateStats(statsData.data);
                updateCharts(statsData.data);
            }
        }

        if (topRes.ok) {
            const topData = await topRes.json();
            if (topData.success) {
                updateTable(topData.data.proxies);
            }
        }

        document.getElementById('lastUpdate').textContent =
            `${I18N.t('dashboard.last_update')}: ${TZ.fmtTime(new Date())}`;
    } catch (err) {
        console.error('Failed to fetch data:', err);
    }
}

function initTopGroupFilter() {
    const sel = document.getElementById('topGroupFilter');
    if (!sel) return;
    sel.addEventListener('change', () => {
        topProxyGroup = sel.value || 'all';
        fetchAllData();
    });
}

async function loadTopProxyGroups() {
    const sel = document.getElementById('topGroupFilter');
    if (!sel) return;
    try {
        const resp = await authFetch(`${API_BASE}/proxy/groups`);
        const json = await resp.json();
        if (!json.success) return;
        const groups = Array.from(new Set(['default', ...(json.data || [])]));
        sel.innerHTML = '<option value="all">ALL</option>' + groups.map(g => `<option value="${escapeHtml(g)}">${escapeHtml(g)}</option>`).join('');
        sel.value = topProxyGroup;
    } catch (_) {
        sel.innerHTML = '<option value="all">ALL</option>';
    }
}

// ─── Update Stats Cards ───
function updateStats(stats) {
    animateValue('totalProxies', stats.total_proxies);
    animateValue('aliveProxies', stats.alive_proxies);
    animateValue('deadProxies', stats.dead_proxies);

    const avgScore = stats.avg_score ? stats.avg_score.toFixed(1) : '0';
    document.getElementById('avgScore').textContent = avgScore;

    const avgLatency = stats.avg_latency_ms ? `${stats.avg_latency_ms.toFixed(0)}ms` : '—';
    document.getElementById('avgLatency').textContent = avgLatency;

    // Alive percentage
    const pct = stats.total_proxies > 0
        ? ((stats.alive_proxies / stats.total_proxies) * 100).toFixed(1) + '%'
        : '—';
    document.getElementById('alivePercentage').textContent = pct;

    // Progress bars
    const maxTotal = Math.max(stats.total_proxies, 1);
    document.getElementById('totalBar').style.width = '100%';
    document.getElementById('aliveBar').style.width =
        `${(stats.alive_proxies / maxTotal) * 100}%`;
    document.getElementById('deadBar').style.width =
        `${(stats.dead_proxies / maxTotal) * 100}%`;
    document.getElementById('scoreBar').style.width = `${stats.avg_score || 0}%`;

    const latencyPct = stats.avg_latency_ms
        ? Math.min((stats.avg_latency_ms / 2000) * 100, 100)
        : 0;
    document.getElementById('latencyBar').style.width = `${latencyPct}%`;
}

function animateValue(elementId, target) {
    const el = document.getElementById(elementId);
    const current = parseInt(el.textContent) || 0;
    if (current === target) return;

    const duration = 800;
    const steps = 30;
    const stepTime = duration / steps;
    const increment = (target - current) / steps;
    let value = current;
    let step = 0;

    const timer = setInterval(() => {
        step++;
        value += increment;
        el.textContent = Math.round(value).toLocaleString();
        if (step >= steps) {
            el.textContent = target.toLocaleString();
            clearInterval(timer);
        }
    }, stepTime);
}

// ─── Charts ───
function initCharts() {
    // Latency - Bar
    const latencyCtx = document.getElementById('latencyChart').getContext('2d');
    latencyChart = new Chart(latencyCtx, {
        type: 'bar',
        data: {
            labels: [],
            datasets: [{
                label: 'Proxies',
                data: [],
                backgroundColor: createGradientBar(latencyCtx, '#00f0ff', '#7b61ff'),
                borderRadius: 6,
                borderSkipped: false,
                maxBarThickness: 40,
            }],
        },
        options: {
            responsive: true,
            maintainAspectRatio: false,
            plugins: { legend: { display: false } },
            scales: {
                x: {
                    grid: { color: 'rgba(255,255,255,0.03)', drawTicks: false },
                    border: { color: 'rgba(255,255,255,0.06)' },
                    ticks: { font: { size: 10 } },
                },
                y: {
                    grid: { color: 'rgba(255,255,255,0.03)', drawTicks: false },
                    border: { display: false },
                    ticks: { font: { size: 10 } },
                    beginAtZero: true,
                },
            },
        },
    });

    // Protocol - Doughnut
    const protocolCtx = document.getElementById('protocolChart').getContext('2d');
    protocolChart = new Chart(protocolCtx, {
        type: 'doughnut',
        data: {
            labels: [],
            datasets: [{
                data: [],
                backgroundColor: [
                    'rgba(0, 240, 255, 0.5)',
                    'rgba(0, 230, 138, 0.5)',
                    'rgba(123, 97, 255, 0.5)',
                    'rgba(255, 165, 2, 0.5)',
                ],
                borderColor: [
                    'rgba(0, 240, 255, 0.8)',
                    'rgba(0, 230, 138, 0.8)',
                    'rgba(123, 97, 255, 0.8)',
                    'rgba(255, 165, 2, 0.8)',
                ],
                borderWidth: 1,
            }],
        },
        options: {
            responsive: true,
            maintainAspectRatio: false,
            cutout: '55%',
            plugins: {
                legend: {
                    position: 'right',
                    labels: { font: { size: 10 }, padding: 8 },
                },
            },
        },
    });

    // Score - Bar (horizontal)
    const scoreCtx = document.getElementById('scoreChart').getContext('2d');
    scoreChart = new Chart(scoreCtx, {
        type: 'bar',
        data: {
            labels: [],
            datasets: [{
                label: 'Proxies',
                data: [],
                backgroundColor: [
                    'rgba(255, 71, 87, 0.6)',
                    'rgba(255, 165, 2, 0.6)',
                    'rgba(250, 204, 21, 0.6)',
                    'rgba(0, 230, 138, 0.6)',
                    'rgba(0, 200, 220, 0.6)',
                    'rgba(0, 240, 255, 0.6)',
                ],
                borderRadius: 6,
                borderSkipped: false,
                maxBarThickness: 32,
            }],
        },
        options: {
            responsive: true,
            maintainAspectRatio: false,
            indexAxis: 'y',
            plugins: { legend: { display: false } },
            scales: {
                x: {
                    grid: { color: 'rgba(255,255,255,0.03)', drawTicks: false },
                    border: { display: false },
                    ticks: { font: { size: 10 } },
                    beginAtZero: true,
                },
                y: {
                    grid: { display: false },
                    border: { display: false },
                    ticks: { font: { size: 10 } },
                },
            },
        },
    });
}

function createGradientBar(ctx, color1, color2) {
    const gradient = ctx.createLinearGradient(0, 0, 0, 300);
    gradient.addColorStop(0, color1);
    gradient.addColorStop(1, color2);
    return gradient;
}

function updateCharts(stats) {
    // Latency chart
    if (stats.latency_distribution && stats.latency_distribution.length > 0) {
        latencyChart.data.labels = stats.latency_distribution.map(l => l.range);
        latencyChart.data.datasets[0].data = stats.latency_distribution.map(l => l.count);
        latencyChart.update('none');
    }

    // Protocol chart
    if (stats.protocol_distribution && stats.protocol_distribution.length > 0) {
        protocolChart.data.labels = stats.protocol_distribution.map(p => p.protocol.toUpperCase());
        protocolChart.data.datasets[0].data = stats.protocol_distribution.map(p => p.count);
        protocolChart.update('none');
    }

    // Score chart
    if (stats.score_distribution && stats.score_distribution.length > 0) {
        scoreChart.data.labels = stats.score_distribution.map(s => s.range);
        scoreChart.data.datasets[0].data = stats.score_distribution.map(s => s.count);
        scoreChart.update('none');
    }
}

// ─── Proxy Table ───
function updateTable(proxies) {
    const tbody = document.getElementById('proxyTableBody');

    if (!proxies || proxies.length === 0) {
        tbody.innerHTML = `
            <tr><td colspan="8" class="table-empty">
                ${I18N.t('dashboard.no_proxies')}
            </td></tr>
        `;
        return;
    }

    tbody.innerHTML = proxies.map(p => {
        const scoreColor = getScoreColor(p.score);
        const protocolClass = `protocol-${p.protocol.toLowerCase()}`;
        const successRate = p.success_rate !== undefined ? p.success_rate.toFixed(1) : '0.0';
        const successCount = p.success_count !== undefined ? p.success_count : 0;
        const rateColor = p.success_rate >= 80 ? COLORS.green : p.success_rate >= 50 ? COLORS.orange : COLORS.red;

        return `
            <tr>
                <td><span class="proxy-addr">${escapeHtml(p.proxy)}</span></td>
                <td><span class="protocol-badge ${protocolClass}">${escapeHtml(p.protocol)}</span></td>
                <td>${getCountryDisplay(p.country)}</td>
                <td>
                    <div class="score-bar-cell">
                        <div class="score-bar-bg">
                            <div class="score-bar-value" style="width:${p.score}%; background:${scoreColor}"></div>
                        </div>
                        <span class="score-text" style="color:${scoreColor}">${p.score.toFixed(0)}</span>
                    </div>
                </td>
                <td><span class="latency-text">${p.latency_ms > 0 ? p.latency_ms.toFixed(0) + 'ms' : '—'}</span></td>
                <td><span style="color:${rateColor};font-weight:500;font-size:0.82rem;">${successRate}%</span></td>
                <td><span style="color:var(--text-secondary);font-size:0.82rem;">${successCount}</span></td>
                <td>
                    <span class="status-badge ${p.is_alive ? 'status-alive' : 'status-dead'}">
                        <span class="status-badge-dot"></span>
                        ${p.is_alive ? I18N.t('dashboard.status_alive') : I18N.t('dashboard.status_dead')}
                    </span>
                </td>
            </tr>
        `;
    }).join('');
}

// ─── Helpers ───
function getScoreColor(score) {
    if (score >= 80) return COLORS.green;
    if (score >= 60) return COLORS.cyan;
    if (score >= 40) return COLORS.orange;
    return COLORS.red;
}

function getCountryDisplay(country) {
    if (!country || country === 'unknown') return '<span style="color:var(--text-dim)">—</span>';
    const flag = getFlagEmoji(country);
    return `<span class="country-flag">${flag}</span>${country.toUpperCase()}`;
}

function getFlagEmoji(countryCode) {
    if (!countryCode || countryCode.length !== 2) return '🌐';
    const code = countryCode.toUpperCase();
    return String.fromCodePoint(
        ...[...code].map(c => 0x1F1E6 + c.charCodeAt(0) - 65)
    );
}

function escapeHtml(str) {
    const div = document.createElement('div');
    div.textContent = str;
    return div.innerHTML;
}
