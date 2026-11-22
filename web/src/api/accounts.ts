import {t} from '../i18n'
import {translateError} from '../i18n/errors'
import {get, post} from './transport'

export async function resolveApproval(id: string, allow: boolean) {
 const result = await post<{resolved: boolean; error?: string}>('/api/approvals', {id, allow})
 if (!result.resolved) throw Error(result.error ? translateError(result.error) : t('chat.approvalGone'))
}

export async function openOAuth(id: string) {
 const path = '/api/oauth/' + encodeURIComponent(id)
 if ('__DOMINION_NATIVE__' in window) {
  await post(path + '/open', {})
  return
 }
 const popup = window.open('', 'dominion_oauth', 'width=650,height=780')
 if (!popup) throw Error(t('oauth.popupBlocked'))
 try {
  const {url} = await get<{url: string}>(path + '/start')
  popup.location.replace(url)
 } catch (error) {
  popup.close()
  throw error
 }
}
