import { invoke } from '@tauri-apps/api/tauri'
import { open } from '@tauri-apps/api/dialog'
import './style.css'

type ClientConfig = {
  server_addr: string
  server_cert_path: string
  local_socks_addr: string
  connect_timeout_ms: number
  log_level: string
  tun: TunConfig
}

type TunConfig = {
  enabled: boolean
  helper_path: string
  device_name: string
  primary_interface: string
  ipv4_addr: string
  mtu: number
}

type RuntimeState = {
  running: boolean
  pid: number | null
  message: string
}

type TunState = {
  running: boolean
  pid: number | null
  message: string
}

type DownloadTunHelperResult = {
  helper_path: string
  wintun_path: string | null
  message: string
}

type RuntimePaths = {
  runtime_dir: string
  config_path: string
  cert_path: string
  config_exists: boolean
  cert_exists: boolean
}

type OnboardingState = {
  serverAddrReady: boolean
  certReady: boolean
}

const app = document.querySelector<HTMLDivElement>('#app')
if (!app) {
  throw new Error('app root not found')
}

app.innerHTML = `
  <main class="container">
    <section class="card">
      <h2 class="title">PX 个人代理</h2>
      <p class="sub">只保留最少配置。填好服务端地址，导入证书，然后启动。</p>
      <div class="grid">
        <label>
          <span>服务端地址</span>
        <input id="server_addr" placeholder="1.2.3.4:6666" />
        </label>
        <label>
          <span>本地 SOCKS5</span>
        <input id="local_socks_addr" placeholder="127.0.0.1:7777" />
        </label>
        <label>
          <span>连接超时 (ms)</span>
        <input id="connect_timeout_ms" type="number" min="1000" step="100" />
        </label>
        <label>
          <span>日志级别</span>
        <select id="log_level">
          <option value="error">error</option>
          <option value="warn">warn</option>
          <option value="info" selected>info</option>
          <option value="debug">debug</option>
          <option value="trace">trace</option>
        </select>
        </label>
      </div>
      <section class="tun-panel">
        <div class="status-row">
          <strong>TUN 全局 TCP</strong>
          <label class="check-row">
            <input id="tun_enabled" type="checkbox" />
            <span>启用</span>
          </label>
        </div>
        <div class="grid">
          <label>
            <span>Helper 路径</span>
            <input id="tun_helper_path" placeholder="bin/tun2socks" />
          </label>
          <label>
            <span>TUN 网卡名</span>
            <input id="tun_device_name" placeholder="utun233 / wintun" />
          </label>
          <label>
            <span>主网卡</span>
            <input id="tun_primary_interface" placeholder="留空自动探测" />
          </label>
          <label>
            <span>TUN IPv4</span>
            <input id="tun_ipv4_addr" placeholder="198.18.0.1" />
          </label>
          <label>
            <span>TUN MTU</span>
            <input id="tun_mtu" type="number" min="1200" step="10" />
          </label>
        </div>
        <div class="actions compact-actions">
          <button id="download-tun-helper" class="secondary">下载 helper</button>
          <button id="start-tun" class="secondary">启动 TUN</button>
          <button id="stop-tun" class="secondary">停止 TUN</button>
        </div>
      </section>
      <div class="actions">
        <button id="import-cert">导入证书</button>
        <button id="save-config">保存配置</button>
        <button id="start-client" class="primary-action">启动</button>
        <button id="stop-client" class="danger">停止</button>
        <button id="run-test" class="secondary">连通性测试</button>
        <button id="open-config-dir" class="secondary">打开配置目录</button>
      </div>
      <div id="test-result" class="status-summary connectivity-summary">尚未执行连通性测试。</div>
      <div id="onboarding-state" class="status-summary">正在检查启动条件...</div>
      <div id="config-dirty-state" class="callout subtle" hidden>当前配置已保存。</div>
    </section>

    <section class="card">
      <h2 class="title">运行状态与日志</h2>
      <div class="status-grid">
        <div class="status-item" id="status-running">运行状态：检查中</div>
        <div class="status-item" id="status-listen">本地地址：--</div>
        <div class="status-item" id="status-cert">证书状态：--</div>
        <div class="status-item" id="status-tun">TUN：--</div>
        <div class="status-item status-item-full" id="status-config">配置文件：--</div>
      </div>
      <div class="actions compact-actions">
        <button id="refresh-status" class="secondary">刷新状态</button>
        <button id="refresh-logs" class="secondary">刷新日志</button>
        <button id="clear-logs" class="secondary">清空日志</button>
      </div>
      <div id="status-summary" class="status-summary">状态摘要：正在读取...</div>
      <div id="runtime-status-message" class="callout subtle">正在读取运行状态...</div>
      <pre id="runtime-logs" class="logs">尚无日志。</pre>
    </section>
  </main>
`

const fields = {
  server_addr: document.querySelector<HTMLInputElement>('#server_addr')!,
  local_socks_addr: document.querySelector<HTMLInputElement>('#local_socks_addr')!,
  connect_timeout_ms: document.querySelector<HTMLInputElement>('#connect_timeout_ms')!,
  log_level: document.querySelector<HTMLSelectElement>('#log_level')!,
  tun_enabled: document.querySelector<HTMLInputElement>('#tun_enabled')!,
  tun_helper_path: document.querySelector<HTMLInputElement>('#tun_helper_path')!,
  tun_device_name: document.querySelector<HTMLInputElement>('#tun_device_name')!,
  tun_primary_interface: document.querySelector<HTMLInputElement>('#tun_primary_interface')!,
  tun_ipv4_addr: document.querySelector<HTMLInputElement>('#tun_ipv4_addr')!,
  tun_mtu: document.querySelector<HTMLInputElement>('#tun_mtu')!,
}
const statusRunning = document.querySelector<HTMLDivElement>('#status-running')!
const statusListen = document.querySelector<HTMLDivElement>('#status-listen')!
const statusConfig = document.querySelector<HTMLDivElement>('#status-config')!
const statusCert = document.querySelector<HTMLDivElement>('#status-cert')!
const statusTun = document.querySelector<HTMLDivElement>('#status-tun')!
const statusSummary = document.querySelector<HTMLDivElement>('#status-summary')!
const runtimeStatusMessage = document.querySelector<HTMLDivElement>('#runtime-status-message')!
const testResult = document.querySelector<HTMLPreElement>('#test-result')!
const runtimeLogs = document.querySelector<HTMLPreElement>('#runtime-logs')!
const onboardingState = document.querySelector<HTMLDivElement>('#onboarding-state')!
const configDirtyState = document.querySelector<HTMLDivElement>('#config-dirty-state')!
const importCertButton = document.querySelector<HTMLButtonElement>('#import-cert')!
const saveConfigButton = document.querySelector<HTMLButtonElement>('#save-config')!
const startClientButton = document.querySelector<HTMLButtonElement>('#start-client')!
const stopClientButton = document.querySelector<HTMLButtonElement>('#stop-client')!
const downloadTunHelperButton = document.querySelector<HTMLButtonElement>('#download-tun-helper')!
const startTunButton = document.querySelector<HTMLButtonElement>('#start-tun')!
const stopTunButton = document.querySelector<HTMLButtonElement>('#stop-tun')!
const runTestButton = document.querySelector<HTMLButtonElement>('#run-test')!
let lastSavedConfig: ClientConfig | null = null
let currentRuntimeState: RuntimeState | null = null
let currentRuntimePaths: RuntimePaths | null = null
let currentTunState: TunState | null = null
let downloadingTunHelper = false
let connectivityTestPending = false
let statusPollTimer: number | null = null
let statusPollInFlight = false
let logPollTimer: number | null = null
let logPollInFlight = false
const STATUS_POLL_INTERVAL_MS = 1000
const LOG_POLL_INTERVAL_MS = 1000

function setConfig(config: ClientConfig) {
  fields.server_addr.value = config.server_addr
  fields.local_socks_addr.value = config.local_socks_addr
  fields.connect_timeout_ms.value = String(config.connect_timeout_ms)
  fields.log_level.value = config.log_level
  fields.tun_enabled.checked = config.tun.enabled
  fields.tun_helper_path.value = config.tun.helper_path
  fields.tun_device_name.value = config.tun.device_name
  fields.tun_primary_interface.value = config.tun.primary_interface
  fields.tun_ipv4_addr.value = config.tun.ipv4_addr
  fields.tun_mtu.value = String(config.tun.mtu)
}

function getConfig(): ClientConfig {
  return {
    server_addr: fields.server_addr.value.trim(),
    server_cert_path: 'config/server-cert.pem',
    local_socks_addr: fields.local_socks_addr.value.trim(),
    connect_timeout_ms: Number(fields.connect_timeout_ms.value || '5000'),
    log_level: fields.log_level.value.trim() || 'info',
    tun: {
      enabled: fields.tun_enabled.checked,
      helper_path: fields.tun_helper_path.value.trim(),
      device_name: fields.tun_device_name.value.trim(),
      primary_interface: fields.tun_primary_interface.value.trim(),
      ipv4_addr: fields.tun_ipv4_addr.value.trim(),
      mtu: Number(fields.tun_mtu.value || '1500'),
    },
  }
}

function configsEqual(left: ClientConfig | null, right: ClientConfig): boolean {
  if (!left) return true
  return JSON.stringify(left) === JSON.stringify(right)
}

function renderDirtyState() {
  const dirty = !configsEqual(lastSavedConfig, getConfig())
  saveConfigButton.classList.toggle('attention-action', dirty)

  if (!dirty) {
    configDirtyState.hidden = true
    return
  }

  configDirtyState.hidden = false
  if (currentRuntimeState?.running) {
    configDirtyState.textContent = '配置已改动，请先保存配置，然后停止并重新启动，修改才会生效。'
    configDirtyState.className = 'callout warning'
  } else {
    configDirtyState.textContent = '配置已改动，请先保存配置。'
    configDirtyState.className = 'callout subtle'
  }
}

function renderConnectivityAction() {
  const shouldHighlight = currentRuntimeState?.running === true && connectivityTestPending
  runTestButton.classList.toggle('attention-action', shouldHighlight)
  runTestButton.classList.toggle('secondary', !shouldHighlight)
}

function renderState(state: RuntimeState) {
  currentRuntimeState = state
  if (!state.running) {
    connectivityTestPending = false
  }
  const statusLabel = state.running ? '运行中' : '未启动'
  const localAddr = fields.local_socks_addr.value.trim() || '未设置'
  const configPath = currentRuntimePaths?.config_path || '未读取'
  const certText = currentRuntimePaths?.cert_exists ? '证书状态：已存在' : '证书状态：未导入'
  const summaryParts = [
    `运行中=${state.running ? '是' : '否'}`,
    `TUN=${currentTunState?.running ? '是' : '否'}`,
    `本地地址=${localAddr}`,
    `配置文件=${configPath}`,
    `证书=${currentRuntimePaths?.cert_exists ? '已导入' : '未导入'}`,
    `未保存配置=${!configsEqual(lastSavedConfig, getConfig()) ? '是' : '否'}`,
  ]

  statusRunning.textContent = `运行状态：${statusLabel}`
  statusRunning.className = `status-item ${state.running ? 'status-ok' : 'status-warn'}`
  statusListen.textContent = `本地地址：${localAddr}`
  statusConfig.textContent = `配置文件：${configPath}`
  statusCert.textContent = certText
  runtimeStatusMessage.textContent = state.message
  runtimeStatusMessage.className = `callout ${state.running ? 'success' : 'subtle'}`
  statusSummary.textContent = `状态摘要：${summaryParts.join(' | ')}`
  statusSummary.className = `status-summary ${state.running ? 'status-summary-ok' : 'status-summary-warn'}`

  startClientButton.disabled = state.running
  stopClientButton.disabled = !state.running
  startClientButton.classList.toggle('primary-action', !state.running)
  startClientButton.classList.toggle('secondary', state.running)
  stopClientButton.classList.toggle('primary-action', state.running)
  stopClientButton.classList.toggle('secondary', !state.running)
  updateStatusPolling()
  updateLogPolling()
  renderDirtyState()
  renderConnectivityAction()
}

function renderTunState(state: TunState) {
  currentTunState = state
  const enabled = fields.tun_enabled.checked
  const statusLabel = state.running ? '运行中' : '未启动'
  statusTun.textContent = `TUN：${enabled ? statusLabel : '未启用'}`
  statusTun.className = `status-item ${state.running ? 'status-ok' : enabled ? 'status-warn' : ''}`.trim()
  downloadTunHelperButton.disabled = state.running || downloadingTunHelper
  startTunButton.disabled = !enabled || state.running
  stopTunButton.disabled = !state.running
  updateStatusPolling()
  updateLogPolling()
}

function renderLogs(logs: string) {
  runtimeLogs.textContent = logs.trim().length > 0 ? logs : '尚无日志。'
}

function toErrorMessage(error: unknown): string {
  const text = String(error)

  if (text.includes('未找到 TUN helper')) {
    return '未找到 tun2socks。请先点击“下载 helper”，或把 tun2socks 放到当前运行目录的 bin/ 中。'
  }
  if (text.includes('未找到 wintun.dll')) {
    return '未找到 wintun.dll。请先点击“下载 helper”，或把官方 wintun.dll 放到当前运行目录的 bin/ 中。'
  }
  if (text.includes('Connection refused')) {
    return '连接被拒绝，请确认客户端已启动，且本地 SOCKS5 地址可访问。'
  }
  if (text.includes('timed out') || text.includes('超时')) {
    return '连接超时，请检查本地代理状态、证书和服务端可达性。'
  }
  if (text.includes('No such file') || text.includes('not found')) {
    return '未找到所需文件，请检查客户端核心程序、配置文件或证书文件是否存在。'
  }
  if (text.includes('Permission denied')) {
    return '权限不足，请检查程序和目标目录的访问权限。'
  }
  return text
}

function onboardingStatus(config: ClientConfig, paths: RuntimePaths): OnboardingState {
  return {
    serverAddrReady: config.server_addr.trim().length > 0,
    certReady: paths.cert_exists,
  }
}

function renderOnboarding(config: ClientConfig, paths: RuntimePaths) {
  const state = onboardingStatus(config, paths)
  const hints: string[] = []

  if (!state.serverAddrReady) {
    hints.push('缺少服务端地址')
  }
  if (!state.certReady) {
    hints.push('缺少证书')
  }

  if (hints.length === 0) {
    onboardingState.textContent = '启动条件已满足，可以直接启动。'
    onboardingState.className = 'status-summary status-summary-ok'
  } else {
    onboardingState.textContent = `启动前还需要：${hints.join('、')}`
    onboardingState.className = 'status-summary status-summary-warn'
  }

  const canStart = state.serverAddrReady && state.certReady
  importCertButton.classList.toggle('attention-action', !state.certReady)
  startClientButton.disabled = !canStart
  startTunButton.disabled = !canStart || !fields.tun_enabled.checked || currentTunState?.running === true
  runTestButton.disabled = !canStart
  renderConnectivityAction()
}

async function refreshConfig() {
  const config = await invoke<ClientConfig>('load_client_config_command')
  setConfig(config)
  lastSavedConfig = config
  renderDirtyState()
  return config
}

async function refreshPaths() {
  const paths = await invoke<RuntimePaths>('runtime_paths_command')
  currentRuntimePaths = paths
  if (currentRuntimeState) {
    renderState(currentRuntimeState)
  }
  return paths
}

async function refreshState() {
  const state = await invoke<RuntimeState>('runtime_state')
  renderState(state)
  return state
}

async function refreshTunState() {
  const state = await invoke<TunState>('tun_state')
  renderTunState(state)
  return state
}

async function refreshLogs() {
  const logs = await invoke<string>('runtime_logs_command')
  renderLogs(logs)
  return logs
}

function shouldPollLogs() {
  return Boolean(currentRuntimeState?.running || currentTunState?.running || downloadingTunHelper)
}

function shouldPollStatus() {
  return Boolean(currentRuntimeState?.running || currentTunState?.running)
}

function startStatusPolling() {
  if (statusPollTimer !== null) return
  statusPollTimer = window.setInterval(async () => {
    if (statusPollInFlight) return
    statusPollInFlight = true
    try {
      await Promise.all([refreshState(), refreshTunState()])
    } finally {
      statusPollInFlight = false
    }
  }, STATUS_POLL_INTERVAL_MS)
}

function stopStatusPolling() {
  if (statusPollTimer === null) return
  window.clearInterval(statusPollTimer)
  statusPollTimer = null
}

function updateStatusPolling() {
  if (shouldPollStatus()) {
    startStatusPolling()
  } else {
    stopStatusPolling()
  }
}

function startLogPolling() {
  if (logPollTimer !== null) return
  logPollTimer = window.setInterval(async () => {
    if (logPollInFlight) return
    logPollInFlight = true
    try {
      await refreshLogs()
    } finally {
      logPollInFlight = false
    }
  }, LOG_POLL_INTERVAL_MS)
}

function stopLogPolling() {
  if (logPollTimer === null) return
  window.clearInterval(logPollTimer)
  logPollTimer = null
}

function updateLogPolling() {
  if (shouldPollLogs()) {
    startLogPolling()
  } else {
    stopLogPolling()
  }
}

async function saveConfig() {
  await invoke('save_client_config_command', { config: getConfig() })
  lastSavedConfig = getConfig()
  renderDirtyState()
}

async function refreshAll() {
  const [config, paths, state] = await Promise.all([
    refreshConfig(),
    refreshPaths(),
    refreshState(),
    refreshTunState(),
    refreshLogs(),
  ])
  renderOnboarding(config, paths)
  return state
}

function failValidation(): boolean {
  const config = getConfig()
  if (!config.server_addr) {
    testResult.textContent = '请先填写服务端地址，再启动客户端。'
    return true
  }
  return false
}

document.querySelector<HTMLButtonElement>('#save-config')!.addEventListener('click', async () => {
  await saveConfig()
  const paths = await refreshPaths()
  renderOnboarding(getConfig(), paths)
  testResult.textContent = '配置已保存。'
})

document.querySelector<HTMLButtonElement>('#import-cert')!.addEventListener('click', async () => {
  const selected = await open({
    multiple: false,
    filters: [{ name: 'PEM', extensions: ['pem', 'crt', 'cer'] }],
  })
  if (!selected || Array.isArray(selected)) {
    return
  }

  try {
    const config = await invoke<ClientConfig>('import_server_cert_command', {
      sourcePath: selected,
    })
    setConfig(config)
    lastSavedConfig = config
    const paths = await refreshPaths()
    renderOnboarding(config, paths)
    renderDirtyState()
    testResult.textContent = '证书已导入到当前运行目录下的 config/server-cert.pem。'
  } catch (error) {
    testResult.textContent = toErrorMessage(error)
  }
})

document.querySelector<HTMLButtonElement>('#start-client')!.addEventListener('click', async () => {
  try {
    if (failValidation()) return
    await saveConfig()
    const paths = await refreshPaths()
    renderOnboarding(getConfig(), paths)
    const state = await invoke<RuntimeState>('start_client')
    renderState(state)
    await refreshTunState()
    await refreshLogs()
    connectivityTestPending = state.running
    renderConnectivityAction()
    testResult.textContent = '配置已保存，客户端已启动。'
  } catch (error) {
    testResult.textContent = toErrorMessage(error)
  }
})

document.querySelector<HTMLButtonElement>('#stop-client')!.addEventListener('click', async () => {
  const state = await invoke<RuntimeState>('stop_client')
  connectivityTestPending = false
  renderState(state)
  await refreshTunState()
  await refreshLogs()
  testResult.textContent = '客户端已停止。'
})

document.querySelector<HTMLButtonElement>('#download-tun-helper')!.addEventListener('click', async () => {
  try {
    downloadingTunHelper = true
    renderTunState(currentTunState ?? { running: false, pid: null, message: 'TUN 未启动' })
    testResult.textContent = '正在下载 TUN helper...'
    const result = await invoke<DownloadTunHelperResult>('download_tun_helper_command')
    fields.tun_helper_path.value = result.helper_path
    await saveConfig()
    await refreshTunState()
    testResult.textContent = result.message
  } catch (error) {
    testResult.textContent = toErrorMessage(error)
  } finally {
    downloadingTunHelper = false
    renderTunState(currentTunState ?? { running: false, pid: null, message: 'TUN 未启动' })
  }
})

document.querySelector<HTMLButtonElement>('#start-tun')!.addEventListener('click', async () => {
  try {
    if (failValidation()) return
    await saveConfig()
    const state = await invoke<TunState>('start_tun')
    renderTunState(state)
    await refreshState()
    testResult.textContent = 'TUN 已启动。'
  } catch (error) {
    testResult.textContent = toErrorMessage(error)
  }
})

document.querySelector<HTMLButtonElement>('#stop-tun')!.addEventListener('click', async () => {
  try {
    const state = await invoke<TunState>('stop_tun')
    renderTunState(state)
    await refreshState()
    testResult.textContent = 'TUN 已停止。'
  } catch (error) {
    testResult.textContent = toErrorMessage(error)
  }
})

document.querySelector<HTMLButtonElement>('#refresh-status')!.addEventListener('click', refreshState)
document.querySelector<HTMLButtonElement>('#refresh-logs')!.addEventListener('click', refreshLogs)
document.querySelector<HTMLButtonElement>('#clear-logs')!.addEventListener('click', async () => {
  await invoke('clear_runtime_logs_command')
  await refreshLogs()
  testResult.textContent = '日志已清空。'
})
document.querySelector<HTMLButtonElement>('#open-config-dir')!.addEventListener('click', async () => {
  try {
    await invoke('open_config_dir_command')
    testResult.textContent = '已尝试打开当前运行目录下的 config 目录。'
  } catch (error) {
    testResult.textContent = toErrorMessage(error)
  }
})

document.querySelector<HTMLButtonElement>('#run-test')!.addEventListener('click', async () => {
  if (failValidation()) return
  testResult.textContent = '测试中...'
  try {
    const result = await invoke<string>('test_proxy_connectivity')
    connectivityTestPending = false
    renderConnectivityAction()
    testResult.textContent = result
  } catch (error) {
    connectivityTestPending = false
    renderConnectivityAction()
    testResult.textContent = toErrorMessage(error)
  }
})

Object.values(fields).forEach((field) => {
  const handleFieldChange = async () => {
    connectivityTestPending = false
    const paths = await refreshPaths()
    renderOnboarding(getConfig(), paths)
    renderDirtyState()
    renderConnectivityAction()
  }

  field.addEventListener('input', handleFieldChange)
  field.addEventListener('change', handleFieldChange)
})

refreshAll().catch((error) => {
  statusRunning.textContent = '运行状态：异常'
  statusRunning.className = 'status-item status-err'
  statusListen.textContent = '本地地址：--'
  statusConfig.textContent = '配置文件：--'
  statusCert.textContent = '证书状态：--'
  statusTun.textContent = 'TUN：--'
  statusTun.className = 'status-item status-err'
  runtimeStatusMessage.textContent = toErrorMessage(error)
  runtimeStatusMessage.className = 'callout warning'
  statusSummary.textContent = `状态摘要：${toErrorMessage(error)}`
  statusSummary.className = 'status-summary status-summary-err'
  currentRuntimeState = null
  currentRuntimePaths = null
  currentTunState = null
  startClientButton.classList.add('primary-action')
  stopClientButton.classList.add('secondary')
  runtimeLogs.textContent = toErrorMessage(error)
  onboardingState.textContent = toErrorMessage(error)
  onboardingState.className = 'status-summary status-summary-err'
  testResult.textContent = toErrorMessage(error)
})
