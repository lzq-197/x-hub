/** @param {string} version */
export function stripBuildMeta(version) {
  const i = version.indexOf('+')
  return i === -1 ? version : version.slice(0, i)
}

/** @param {number} n */
function pad2(n) {
  return String(n).padStart(2, '0')
}

/**
 * @param {string} baseVersion
 * @param {Date} [date]
 */
export function prodStamp(baseVersion, date = new Date()) {
  const base = stripBuildMeta(baseVersion)
  const meta = `${pad2(date.getMonth() + 1)}${pad2(date.getDate())}${pad2(date.getHours())}${pad2(date.getMinutes())}`
  return `${base}+${meta}`
}

/** @param {string} baseVersion */
export function releaseStamp(baseVersion) {
  return stripBuildMeta(baseVersion)
}

const VERSION_RE = /("version"\s*:\s*")([^"]*)(")/

/** @param {string} confText */
export function readConfVersion(confText) {
  const m = confText.match(VERSION_RE)
  if (!m) throw new Error('tauri.conf.json: missing "version" field')
  return m[2]
}

/**
 * @param {string} confText
 * @param {string} version
 */
export function writeConfVersion(confText, version) {
  if (!VERSION_RE.test(confText)) {
    throw new Error('tauri.conf.json: missing "version" field')
  }
  return confText.replace(VERSION_RE, `$1${version}$3`)
}
