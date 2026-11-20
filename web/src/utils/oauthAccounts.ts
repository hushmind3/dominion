/** Account connectors still supported by the Rust OAuth registry.
 * Never infer a provider's identity from the presence of a saved token:
 * discontinued integrations can remain in a pre-upgrade database. */
export type OAuthAccountId = 'github' | 'google'

export const isOAuthAccountId = (id: string): id is OAuthAccountId =>
  id === 'github' || id === 'google'

export function supportedOAuthAccounts<T extends {id: string}>(
  providers: readonly T[]
): Array<T & {id: OAuthAccountId}> {
  return providers.filter(
    (provider): provider is T & {id: OAuthAccountId} => isOAuthAccountId(provider.id)
  )
}
