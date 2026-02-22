import { fetchEventSource } from '@microsoft/fetch-event-source'
import { client } from './client'
import type {
  CatalogResponse,
  DiscoveryEventType,
  DiscoveryRequest,
  JobStatusResponse,
  SubmitDiscoveryResponse,
} from '../types/catalog'

function getToken(): string {
  return localStorage.getItem('token') ?? ''
}

// Submit a discovery job and stream its SSE events until result/error/done.
// onProgress is called for each progress event.
// Returns the result data once a "result" event is received.
// Throws on error or cancellation.
export async function submitAndStream<T>(
  datasourceId: string,
  request: DiscoveryRequest,
  onProgress: (phase: string, detail: string) => void,
  signal?: AbortSignal,
): Promise<T> {
  // Step 1: Submit job → get job_id
  const { data: submitData } = await client.post<SubmitDiscoveryResponse>(
    `/datasources/${datasourceId}/discover`,
    request,
  )
  const jobId = submitData.job_id

  // Step 2: Stream SSE events
  return new Promise<T>((resolve, reject) => {
    fetchEventSource(`/api/v1/datasources/${datasourceId}/discover/${jobId}/events`, {
      headers: {
        Authorization: `Bearer ${getToken()}`,
      },
      signal,
      onmessage(ev) {
        let event: DiscoveryEventType
        try {
          event = JSON.parse(ev.data) as DiscoveryEventType
        } catch {
          return
        }

        if (event.type === 'progress') {
          onProgress(event.phase, event.detail)
        } else if (event.type === 'result') {
          resolve(event.data as T)
        } else if (event.type === 'error') {
          reject(new Error(event.message))
        } else if (event.type === 'cancelled') {
          reject(new Error('Discovery cancelled'))
        }
        // 'done' without a result is a no-op (result was already resolved)
      },
      onerror(err) {
        reject(err)
      },
    })
  })
}

// Cancel a running discovery job.
export async function cancelDiscovery(datasourceId: string, jobId: string): Promise<void> {
  await client.delete(`/datasources/${datasourceId}/discover/${jobId}`)
}

// Poll job status (fallback for environments where SSE isn't reliable).
export async function getDiscoveryStatus(
  datasourceId: string,
  jobId: string,
): Promise<JobStatusResponse> {
  const { data } = await client.get<JobStatusResponse>(
    `/datasources/${datasourceId}/discover/${jobId}`,
  )
  return data
}

// Read stored catalog (fast local DB read, no upstream connection).
export async function getCatalog(datasourceId: string): Promise<CatalogResponse> {
  const { data } = await client.get<CatalogResponse>(`/datasources/${datasourceId}/catalog`)
  return data
}
