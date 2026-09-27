/** @param {{ buildFailed: unknown, restoreFailed: boolean, destPath: string | null | undefined }} p */
export function shouldOpenArtifactDir({ buildFailed, restoreFailed, destPath }) {
  return !buildFailed && !restoreFailed && Boolean(destPath)
}

/** @param {string} platform @param {string} dir */
export function artifactOpenCommand(platform, dir) {
  if (platform === 'win32') return { cmd: 'explorer', args: [dir] }
  if (platform === 'darwin') return { cmd: 'open', args: [dir] }
  return { cmd: 'xdg-open', args: [dir] }
}
