import axios from 'axios'

export interface VersionInfo {
  current: string
}

export async function getVersion(): Promise<VersionInfo> {
  const { data } = await axios.get<{ version: string }>('/health')
  return { current: data.version }
}
