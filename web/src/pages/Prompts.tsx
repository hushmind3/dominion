import {PageContainer} from '../components/Primitives'
import {SystemPromptPanel} from './memory/SystemPromptPanel'
import {t, useLocale} from '../i18n'

export function PromptsPage() {
 useLocale()
 return <PageContainer>
  <p className="mb-4 max-w-3xl text-sm leading-7 text-muted">{t('prompts.intro')}</p>
  <SystemPromptPanel/>
 </PageContainer>
}
