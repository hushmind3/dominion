import {useRemote} from './useRemote'
import type {ModelOption} from '../api'

export function useProviderModels(providerId: string, active = true) {
 return useRemote<ModelOption[]>(providerId ? '/api/providers/' + encodeURIComponent(providerId) + '/models' : null, [], active)
}
