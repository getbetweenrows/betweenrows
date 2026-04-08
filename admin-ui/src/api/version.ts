import axios from 'axios'

export interface VersionInfo {
  current: string
  commit: string
}

export async function getVersion(): Promise<VersionInfo> {
  const { data } = await axios.get<{ version: string; commit: string }>('/health')
  return { current: data.version, commit: data.commit }
}
