import { koreanToolSummary } from '../lib/toolSummary'
import {discoveryForMcp,registryLaunch,type RegistryMcp} from '../features/mcp/quickCommands'
import {DialogBackdrop,DialogPanel} from "../components/Dialog";
import {SkillsPanel} from "./extensions/SkillsPanel";
import {McpRegistryPreview} from './extensions/McpRegistryPreview';
import {PluginDiscovery} from "./extensions/PluginDiscovery";
import {CapabilitiesPage} from './Capabilities';
import {t,useLocale} from "../i18n";
import {
  Button,
  TextInput,
  TextArea,
  SelectInput,
} from "../components/Primitives";
import {useEffect,useRef,useState} from "react";
import {
  Activity,
  ArrowDownToLine,
  ArrowRight,
  Blocks,
  BookOpenText,
  Check,
  ChevronRight,
  Code2,
  Compass,
  Download,
  ExternalLink,
  FileCode2,
  Globe2,
  HardDrive,
  LoaderCircle,
  PackagePlus,
  Plus,
  RefreshCw,
  Search,
  Server,
  Settings2,
  ShieldCheck,
  Sparkles,
  TerminalSquare,
  Trash2,
  X,
} from "lucide-react";
import {del,get,post} from "../api";
import {
  Action,
  Empty,
  FormField,
  Headline,
  Notice,
  Panel,
  StateTag,
  Subheading,
  refresh,
  useRemote,
} from "../components/UI";
type Mcp = {
  id: string;
  name: string;
  transport: "http" | "stdio";
  url?: string;
  command?: string;
  args?: string[];
  enabled: boolean;
};
type Skill = {
  id: string;
  name: string;
  instructions: string;
  enabled: boolean;
  status?: string;
  source?: string;
};
type RegistryItem = RegistryMcp;
const featured = [
  {
    name: "Playwright Browser",
    description: "웹 페이지 탐색, 클릭, 입력 및 스크린샷",
    descriptionEn: "Browse web pages, click, type and capture screenshots",
    icon: Globe2,
    transport: "stdio",
    command: "npx",
    args: ["-y", "@playwright/mcp@latest"],
    url: "https://playwright.dev/mcp/installation",
  },
  {
    name: "Hugging Face",
    description: "모델·데이터셋을 검색하는 원격 MCP 서버",
    descriptionEn: "Remote MCP for searching models and datasets",
    icon: BrainIcon,
    transport: "http",
    command: "",
    args: [],
    endpoint: "https://huggingface.co/mcp",
    url: "https://huggingface.co",
  },
];
function BrainIcon({ size = 22 }: { size?: number }) {
  return <Sparkles size={size} />;
}
const skillTemplates = [
  {
    name: "웹 리서치",
    nameEn: "Web research",
    category: "리서치",
    categoryEn: "Research",
    instructions:
      "검색이 필요한 요청은 web_search를 먼저 호출하고, 서로 다른 출처 2~3개를 비교한 뒤 URL과 함께 요약한다. 확인되지 않은 내용은 추측이라고 명시한다.",
    instructionsEn:
      "For requests that need search, call web_search first, compare two or three independent sources, and summarize with URLs. State clearly when something is unverified and is an inference.",
  },
  {
    name: "리서치 노트",
    nameEn: "Research notes",
    category: "리서치",
    categoryEn: "Research",
    instructions:
      "조사한 내용을 주장·근거·출처·추가 확인 항목으로 나눠 노트로 정리한다. 출처가 없는 주장은 별도 표시한다.",
    instructionsEn:
      "Organize findings into claims, evidence, sources and open questions. Mark claims without a source separately.",
  },
  {
    name: "경쟁사 분석",
    nameEn: "Competitor analysis",
    category: "리서치",
    categoryEn: "Research",
    instructions:
      "제품, 가격, 타깃, 차별점 기준으로 비교표를 만든다. 확인된 사실과 추정을 구분하고 출처 URL을 남긴다.",
    instructionsEn:
      "Build a comparison table of product, pricing, target and differentiators. Separate confirmed facts from estimates and keep source URLs.",
  },
  {
    name: "파일 정리",
    nameEn: "File organization",
    category: "파일",
    categoryEn: "Files",
    instructions:
      "대상 폴더를 file_list로 먼저 확인하고, 이동·이름 변경 전에 현재 구조를 요약한다. 파일을 삭제하기 전에는 항상 사용자에게 확인한다.",
    instructionsEn:
      "Inspect the target folder with file_list first and summarize its structure before moving or renaming anything. Always confirm with the user before deleting files.",
  },
  {
    name: "파일 검색",
    nameEn: "File search",
    category: "파일",
    categoryEn: "Files",
    instructions:
      "파일 이름과 내용을 함께 검색하고, 일치한 경로·크기·수정 시간을 표로 정리한다. 읽기 전용으로 먼저 확인한다.",
    instructionsEn:
      "Search file names and contents together, then report matching paths, sizes and modification times in a table. Start read-only.",
  },
  {
    name: "코드 리뷰",
    nameEn: "Code review",
    category: "코딩",
    categoryEn: "Coding",
    instructions:
      "버그, 보안, 성능, 유지보수성 순서로 리뷰한다. 각 지적에는 파일 경로와 근거를 붙이고, 확실하지 않은 내용은 질문으로 남긴다.",
    instructionsEn:
      "Review in the order of bugs, security, performance and maintainability. Attach a file path and evidence to each finding and leave uncertain points as questions.",
  },
  {
    name: "테스트 작성",
    nameEn: "Write tests",
    category: "코딩",
    categoryEn: "Coding",
    instructions:
      "기존 테스트 프레임워크를 먼저 확인하고, 정상·경계·실패 경로를 각각 검증하는 테스트를 추가한다. 테스트가 실제로 실패하는지도 확인한다.",
    instructionsEn:
      "Check the existing test framework first, then add tests covering success, boundary and failure paths. Confirm the tests actually fail without the fix.",
  },
  {
    name: "리팩터링",
    nameEn: "Refactoring",
    category: "코딩",
    categoryEn: "Coding",
    instructions:
      "동작을 바꾸지 않는 범위에서 중복과 결합을 줄인다. 변경 전후 테스트를 실행하고, 공개 API 변경은 별도로 보고한다.",
    instructionsEn:
      "Reduce duplication and coupling without changing behavior. Run tests before and after, and report public API changes separately.",
  },
  {
    name: "Git 작업",
    nameEn: "Git work",
    category: "개발 운영",
    categoryEn: "DevOps",
    instructions:
      "변경 상태를 git status와 diff로 확인한 뒤 작업한다. 커밋 메시지는 변경 이유를 한 줄로 요약하고, 사용자가 요청하지 않은 push는 하지 않는다.",
    instructionsEn:
      "Check git status and diff before working. Summarize the reason for the change in one commit line and never push unless the user asks.",
  },
  {
    name: "배포 체크리스트",
    nameEn: "Release checklist",
    category: "개발 운영",
    categoryEn: "DevOps",
    instructions:
      "빌드, 테스트, 환경변수, 마이그레이션, 롤백 계획을 순서대로 확인한다. 실패한 항목이 있으면 배포 완료라고 말하지 않는다.",
    instructionsEn:
      "Check build, tests, environment variables, migrations and rollback plan in order. If anything fails, do not call the release complete.",
  },
  {
    name: "로그 분석",
    nameEn: "Log analysis",
    category: "개발 운영",
    categoryEn: "DevOps",
    instructions:
      "에러 빈도와 시간대, 공통 스택 트레이스, 최초 발생 시점을 분리해 분석한다. 원인과 추정을 구분한다.",
    instructionsEn:
      "Separate error frequency and timing, shared stack traces and first occurrence. Distinguish cause from inference.",
  },
  {
    name: "장애 대응",
    nameEn: "Incident response",
    category: "개발 운영",
    categoryEn: "DevOps",
    instructions:
      "영향 범위, 현재 상태, 즉시 완화 조치, 재발 방지 항목 순서로 정리한다. 확인되지 않은 원인을 단정하지 않는다.",
    instructionsEn:
      "Report impact, current state, immediate mitigation and prevention items in that order. Never assert an unverified cause.",
  },
  {
    name: "보안 점검",
    nameEn: "Security review",
    category: "보안",
    categoryEn: "Security",
    instructions:
      "비밀키 노출, 인증 우회, 입력 검증, 권한 범위를 우선 점검한다. 민감한 값을 출력하지 않고 위치와 위험도만 보고한다.",
    instructionsEn:
      "Check secret exposure, auth bypass, input validation and permission scope first. Report locations and risk only, never the sensitive values.",
  },
  {
    name: "데이터 분석",
    nameEn: "Data analysis",
    category: "데이터",
    categoryEn: "Data",
    instructions:
      "질문을 지표와 차원으로 나누고, 데이터 범위와 전처리 가정을 먼저 밝힌다. 결과와 한계를 함께 보고한다.",
    instructionsEn:
      "Split the question into metrics and dimensions, state the data range and preprocessing assumptions, and report results with limitations.",
  },
  {
    name: "SQL 쿼리",
    nameEn: "SQL queries",
    category: "데이터",
    categoryEn: "Data",
    instructions:
      "스키마를 먼저 확인하고 읽기 전용 쿼리로 검증한다. 대량 변경 전에는 예상 행 수와 롤백 방법을 제시한다.",
    instructionsEn:
      "Inspect the schema first and verify with read-only queries. Before bulk changes, state the expected row count and rollback method.",
  },
  {
    name: "문서 작성",
    nameEn: "Writing documents",
    category: "글쓰기",
    categoryEn: "Writing",
    instructions:
      "독자, 목적, 핵심 메시지를 먼저 정하고 제목·요약·본문·다음 단계 순서로 작성한다. 사실과 의견을 구분한다.",
    instructionsEn:
      "Define the audience, purpose and key message first, then write title, summary, body and next steps. Separate facts from opinions.",
  },
  {
    name: "회의 요약",
    nameEn: "Meeting summary",
    category: "글쓰기",
    categoryEn: "Writing",
    instructions:
      "결정 사항, 담당자, 마감, 미해결 질문을 구분해 요약한다. 원문에 없는 담당자나 날짜를 만들어내지 않는다.",
    instructionsEn:
      "Summarize decisions, owners, deadlines and open questions separately. Never invent owners or dates not in the source.",
  },
  {
    name: "번역·현지화",
    nameEn: "Translation and localization",
    category: "글쓰기",
    categoryEn: "Writing",
    instructions:
      "원문의 의미와 톤을 유지하고, 고유명사·UI 문자열은 용어집을 따른다. 직역투를 피하고 변경 이유를 간단히 남긴다.",
    instructionsEn:
      "Keep the meaning and tone of the source, follow the glossary for proper nouns and UI strings, avoid literal phrasing, and note why a change was made.",
  },
  {
    name: "API 문서화",
    nameEn: "API documentation",
    category: "글쓰기",
    categoryEn: "Writing",
    instructions:
      "엔드포인트, 인증, 요청·응답 예시, 오류 코드, 제한 사항을 실제 코드와 대조해 작성한다.",
    instructionsEn:
      "Document endpoints, authentication, request and response examples, error codes and limits against the real code.",
  },
  {
    name: "이메일·메시지",
    nameEn: "Email and messages",
    category: "커뮤니케이션",
    categoryEn: "Communication",
    instructions:
      "목적, 요청 사항, 기한, 다음 단계가 한 번에 보이게 작성한다. 상대에 따라 정중함과 간결함의 균형을 조정한다.",
    instructionsEn:
      "Show purpose, request, deadline and next step at a glance. Adjust the balance of politeness and brevity for the recipient.",
  },
  {
    name: "일정 정리",
    nameEn: "Schedule planning",
    category: "생산성",
    categoryEn: "Productivity",
    instructions:
      "할 일을 중요도·마감·소요 시간으로 나누고 오늘/이번 주/나중으로 묶는다. 일정 충돌을 먼저 확인한다.",
    instructionsEn:
      "Split tasks by importance, deadline and effort, then group them into today, this week and later. Check for conflicts first.",
  },
  {
    name: "프롬프트 설계",
    nameEn: "Prompt design",
    category: "AI",
    categoryEn: "AI",
    instructions:
      "역할, 입력, 제약, 출력 형식, 검증 기준을 명시한 프롬프트를 만든다. 모호한 지시는 예시로 보완한다.",
    instructionsEn:
      "Write prompts that specify role, input, constraints, output format and validation criteria. Clarify vague instructions with examples.",
  },
];
function StatusLine({
  label,
  value,
}: {
  label: string;
  value: string | number;
}) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <i className="size-1.5 rounded-full bg-success" />
      <span className="text-muted">{label}</span>
      <strong className="text-ink">{value}</strong>
    </span>
  );
}
export type ExtensionTab='market'|'installed'|'plugins'|'skills'
export function ExtensionsPage({section,embedded=false,onMcpRegistered}:{section?:ExtensionTab;embedded?:boolean;onMcpRegistered?:()=>void}={}) {
  const loc = useLocale();
  const [localTab, setTab] = useState<ExtensionTab>("installed");
  const [showBuiltins, setShowBuiltins] = useState(false);
  const tab = section ?? localTab;
  // Services embeds only MCP management: do not load unrelated skill data
  // or show the native tool inventory in a connection management screen.
  const mcpOnlyEmbed=embedded&&(tab==='market'||tab==='installed');
  const legacyRuntime=useRemote<{available:boolean|null}>('/api/mcp/runtime',{available:null});
  const mcp = useRemote<Mcp[]>("/api/mcp", []),
    skills = useRemote<Skill[]>(mcpOnlyEmbed?null:"/api/skills", []),
    presets = useRemote<any[]>(tab === "market"&&!mcpOnlyEmbed ? "/api/mcp/presets" : null, []);
  const catalogSkills = useRemote<{
    items: any[];
    total: number;
    updatedAt: string;
  }>(tab === "plugins" ? "/api/skill-catalog" : null, { items: [], total: 0, updatedAt: "" }, false);
  const [repo, setRepo] = useState(""),
    [repoBusy, setRepoBusy] = useState(false),
    [mcpSaving, setMcpSaving] = useState(false),
    [mcpToggling, setMcpToggling] = useState('');
  const [query, setQuery] = useState(""),
    [catalog, setCatalog] = useState<RegistryItem[]>([]),
    [searching, setSearching] = useState(false),
    [installing, setInstalling] = useState(""),
    [error, setError] = useState(""),
    [message, setMessage] = useState("");
  const [skillQuery, setSkillQuery] = useState(""),
    [skillCategory, setSkillCategory] = useState("all");
  const [pluginQuery, setPluginQuery] = useState(""),
    [pluginCategory, setPluginCategory] = useState("all");
  const [catalogQuery, setCatalogQuery] = useState(""),
    [catalogSource, setCatalogSource] = useState("all"),
    [catalogCategory, setCatalogCategory] = useState("all"),
    [catalogLimit, setCatalogLimit] = useState(30),
    [importing, setImporting] = useState("");
  const templates = skillTemplates.map((item) =>
    loc === "en"
      ? {
          ...item,
          name: item.nameEn,
          category: item.categoryEn,
          instructions: item.instructionsEn,
        }
      : item,
  );
  const skillCategories = [
    "all",
    ...new Set(templates.map((item) => item.category)),
  ];
  const filteredSkills = templates.filter(
    (item) =>
      (skillCategory === "all" || item.category === skillCategory) &&
      (item.name + item.instructions + item.category)
        .toLowerCase()
        .includes(skillQuery.toLowerCase()),
  );
  const pluginCategories = [
    "all",
    ...new Set(presets.value.map((p: any) => p.category)),
  ];
  const filteredPresets = presets.value.filter(
    (p: any) =>
      (pluginCategory === "all" || p.category === pluginCategory) &&
      (p.name + p.description + p.category)
        .toLowerCase()
        .includes(pluginQuery.toLowerCase()),
  );
  const catalogItems = catalogSkills.value.items || [];
  const catalogSources = [
    "all",
    ...new Set(catalogItems.map((item: any) => item.source)),
  ];
  const catalogCategories = [
    "all",
    ...new Set(catalogItems.map((item: any) => item.category)),
  ];
  const filteredCatalog = catalogItems.filter(
    (item: any) =>
      (catalogSource === "all" || item.source === catalogSource) &&
      (catalogCategory === "all" || item.category === catalogCategory) &&
      (
        item.name +
        item.description +
        item.category +
        item.source +
        (item.tags || []).join(" ")
      )
        .toLowerCase()
        .includes(catalogQuery.toLowerCase()),
  );
  const visibleCatalog = filteredCatalog.slice(0, catalogLimit);
  const [edit, setEdit] = useState<any | null>(null),
    [argText, setArgText] = useState(""),
    [skillEdit, setSkillEdit] = useState<Partial<Skill> | null>(null),
    [tools, setTools] = useState<any[]>([]);
  const discoveryVersion = useRef(0);
  const install = async (entry: any) => {
    if (
      !confirm(
        t('ext.confirmRegister')+(entry.transport==='stdio'?'\n'+[entry.command,...(entry.args||[])].join(' '):''),
      )
    )
      return;
    setError("");
    setMessage("");
    try {
      const payload = {
        name: entry.name,
        transport: entry.transport || "stdio",
        url: entry.endpoint || entry.url || "",
        command: entry.command || "npx",
        args:
          entry.args || (entry.packageName ? ["-y", entry.packageName] : []),
      };
      await post("/api/mcp", payload);
      await mcp.reload();
      setMessage(t('ext.registeredHint'));
      // In Service connections, take the owner straight to the real installed
      // server and its connection check rather than leaving a dead-end badge.
      onMcpRegistered?.();
    } catch (e) {
      setError((e as Error).message);
    }
  };
  const installPreset = async (preset: any) => {
    if (installing) return;
    setInstalling(preset.id);
    setError("");
    setMessage(
      t('ext.installHint', { name: preset.name }),
    );
    try {
      const saved = await post<any>("/api/mcp", {
        name: preset.name,
        transport: "stdio",
        command: "npx",
        args: ["-y", preset.package, ...(preset.args || [])],
        enabled: true,
      });
      await mcp.reload();
      for (let attempt = 0; attempt < 30; attempt++) {
        const found = await get<any[]>("/api/mcp/discover");
        setTools(found);
        const {tools:own,error:failure}=discoveryForMcp(found,saved.id);
        if (own.length) {
          setMessage(t('ext.connectedToolsMsg', { name: preset.name, count: own.length }));
          return;
        }
        if (failure) {
          setError((loc === 'en' ? 'MCP connection failed: ' : 'MCP 연결 실패: ')+failure);
          return;
        }
        await new Promise((resolve) => setTimeout(resolve, 1000));
      }
      setMessage(t('ext.registeredCheckMsg', { name: preset.name }));
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setInstalling("");
    }
  };
  const loadCatalog = async (term = "") => {
    setSearching(true);
    setError("");
    try {
      setCatalog(
        await get(
          "/api/mcp/catalog" + (term ? "?q=" + encodeURIComponent(term) : ""),
        ),
      );
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSearching(false);
    }
  };
  const search = async () => {
    if (query.trim() && query.trim().length < 3) {
      setError(t('ext.searchMin'));
      return;
    }
    await loadCatalog(query.trim());
  };
  const toggleMcp = async (record:Mcp, enabled:boolean) => {
    if (mcpToggling || mcpSaving) return;
    setMcpToggling(record.id);
    discoveryVersion.current += 1; // Invalidate older discovery responses before changing plugin availability.
    setError('');
    try {
      await post('/api/mcp', {...record, enabled});
      await mcp.reload();
      // A disabled plugin must disappear from the displayed live tool catalog.
      setTools(previous => previous.filter(tool => tool.mcpId !== record.id));
      setMessage(loc === 'en'
        ? `${record.name} ${enabled ? 'enabled' : 'disabled'}`
        : `${record.name} ${enabled ? '사용' : '사용 중지'}`);
      if (enabled && tab === 'installed') void discover(true);
    } catch (e) {
      setError((e as Error).message);
      await mcp.reload();
    } finally {
      setMcpToggling('');
    }
  };
  const saveMcp = async () => {
    if (!edit || mcpSaving) return;
    if (!edit.name?.trim() || (edit.transport === "http"
      ? !/^https?:\/\//i.test(edit.url?.trim() || "")
      : !edit.command?.trim())) {
      setError(loc === "en"
        ? "Enter a name and a valid HTTP address or command."
        : "이름과 올바른 HTTP 주소 또는 실행 명령을 입력하세요.");
      return;
    }
    setMcpSaving(true);
    setError("");
    try {
      await post("/api/mcp", {
        ...edit,
        name: edit.name.trim(),
        url: edit.url?.trim(),
        command: edit.command?.trim(),
        args: argText.split("\n").map((x) => x.trim()).filter(Boolean),
      });
      setEdit(null);
      await mcp.reload();
      if (tab === "installed") await discover(true);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setMcpSaving(false);
    }
  };
  const saveSkill = async () => {
    if (!skillEdit?.name) return;
    try {
      await post("/api/skills", skillEdit);
      setSkillEdit(null);
      skills.reload();
    } catch (e) {
      setError((e as Error).message);
    }
  };
  const addSkillTemplate = async (template: any) => {
    if (skills.value.some((x) => x.name === template.name)) return;
    try {
      await post("/api/skills", { ...template, enabled: true });
      await skills.reload();
      setMessage(t('ext.skillAdded', { name: template.name }));
    } catch (e) {
      setError((e as Error).message);
    }
  };
  const importCatalogSkill = async (item: any) => {
    if (importing) return;
    setImporting(item.id);
    setError("");
    setMessage("");
    try {
      await post("/api/skill-catalog/import", { id: item.id });
      await skills.reload();
      setMessage(t('ext.skillImported', { source: item.source, name: item.name }));
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setImporting("");
    }
  };
  const importRepo = async () => {
    if (!repo.trim()) return;
    setRepoBusy(true);
    setError("");
    setMessage("");
    try {
      const r = await post<any>("/api/skills/import-repo", {
        repo: repo.trim(),
      });
      setMessage(t('ext.skillImportedRepo', { repo: r.repo, count: r.added }));
      await skills.reload();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setRepoBusy(false);
    }
  };
  const discover = async (silent = false) => {
    const revision = ++discoveryVersion.current;
    try {
      const discovered = await get<any[]>("/api/mcp/discover");
      if (revision !== discoveryVersion.current) return;
      setTools(discovered);
      if (!silent) setMessage(t('ext.toolsFound'));
    } catch (e) {
      if (!silent) setError((e as Error).message);
    }
  };
  // Active-tab only: do not run remote MCP discovery from the plugins/skills screens.
  useEffect(() => {
    if (tab === "installed"&&legacyRuntime.value.available===true) void discover(true);
  }, [tab,legacyRuntime.value.available]);
  useEffect(() => {
    if (tab === "market"&&legacyRuntime.value.available===true) void loadCatalog();
  }, [tab,legacyRuntime.value.available]);
  if((tab==='market'||tab==='installed')&&legacyRuntime.value.available!==true){
    const en=loc==='en';
    const retained=mcp.value.map(r=>{const raw=r as unknown as Record<string,unknown>;return Object.fromEntries(['id','name','transport','url','command','args','enabled','protocolMode'].filter(k=>raw[k]!==undefined).map(k=>[k,raw[k]]))});
    return <section className="space-y-4 rounded-xl border border-divider bg-surface p-4">
      <h3 className="font-bold">{en?'Independent MCP extension':'독립 MCP 확장 프로그램'}</h3>
      {legacyRuntime.value.available===null?<p className="text-sm text-muted">{legacyRuntime.error||(en?'Checking runtime…':'실행 환경 확인 중…')}</p>:<>
       <p className="text-sm">{en?'MCP transport is no longer built into the standard application. Install a tool-provider extension and register its tools. Existing records and keys are retained.':'기본 도미니온에 MCP 전송 엔진을 내장하지 않습니다. 확장 프로그램을 설치하고 제공 도구를 등록하세요. 기존 연결 기록과 키는 보존됩니다.'}</p>
       <a href="#/connections?view=plugins&q=MCP" className="inline-flex rounded-lg border border-divider bg-canvas px-3 py-2 text-sm text-brand">{en?'Open extension marketplace':'확장 프로그램 마켓 열기'}</a>
       {!embedded&&<Button variant="secondary" onClick={()=>setTab('plugins')}>{en?'Extensions':'확장 프로그램'}</Button>}
       {tab==='market'&&<McpRegistryPreview/>}
       {retained.length>0&&<details><summary className="cursor-pointer text-sm">{en?'Retained connections (review URLs and arguments before copying)':'보존된 연결 설정 (URL·실행 옵션의 비밀정보 확인 필요)'} · {retained.length}</summary><p className="my-2 text-xs text-muted">{en?'Copy these records into the installed connector settings. Review container-local commands and re-enter credentials; no automatic transfer.':'설치한 연결 확장의 설정에 복사하세요. 컨테이너 내부 실행 명령을 확인하고 키·환경변수는 직접 입력해야 합니다.'}</p><textarea className="w-full rounded-lg border border-divider bg-canvas p-2 font-mono text-xs" readOnly rows={10} value={JSON.stringify({servers:retained},null,2)}/></details>}
      </>}
    </section>
  }
  return (
    <div className={embedded?"min-w-0":"mx-auto w-full max-w-[1320px] px-5 py-4 sm:px-7 lg:px-6 lg:pb-8"}>
      {!embedded && <Headline
        eyebrow={t('ui.eyebrow.marketplace')}
        title={t('ext.title')}
        description={t('ext.description')}
        actions={legacyRuntime.value.available===true ?
          <Action
            variant="secondary"
            onClick={() => {
              setEdit({
                name: "",
                transport: "stdio",
                command: "npx",
                url: "",
                enabled: true,
              });
              setArgText("");
            }}
          >
            <Plus size={17} /> {t('ext.addMcp')}
          </Action> : <Action variant="secondary" onClick={()=>setTab('plugins')}><PackagePlus size={17}/>{loc==='en'?'Extensions':'확장 프로그램'}</Action>
        }
      />}
      {(!embedded||tab==='market'||tab==='installed')&&<div className="mb-4 flex flex-wrap items-center gap-3 rounded-xl border border-divider bg-canvas px-4 py-3 text-[11px] text-muted">
        <StatusLine label={t('ext.tab.servers')} value={mcp.value.length} />
        <StatusLine
          label={t('ext.tab.tools')}
          value={tools.filter((t) => !t.error).length}
        />
        {!mcpOnlyEmbed&&<StatusLine
          label={t('ext.tab.skills')}
          value={
            skills.value.filter((s) => s.enabled).length +
            "/" +
            skills.value.length
          }
        />}
      </div>}
      {!embedded&&<div className="mb-6 flex flex-wrap items-center gap-2 border-b border-divider pb-3">
        <Button
          variant={tab === "market" ? "primary" : "ghost"}
          onClick={() => setTab("market")}
        >
          <Blocks size={17} /> {loc==='en'?'MCP Registry':'MCP 레지스트리'}
        </Button>
        <Button
          variant={tab === "plugins" ? "primary" : "ghost"}
          onClick={() => setTab("plugins")}
        >
          <PackagePlus size={17} /> {t('ext.tab.plugins')}
        </Button>
        <Button
          variant={tab === "installed" ? "primary" : "ghost"}
          onClick={() => setTab("installed")}
        >
          {t('ext.tab.installed')} <b>{mcp.value.length}</b>
        </Button>
        <Button
          variant={tab === "skills" ? "primary" : "ghost"}
          onClick={() => setTab("skills")}
        >
          <BookOpenText size={17} /> {t('ext.tab.skills')}{" "}
          <b>
            {skills.value.filter((s) => s.enabled).length}/{skills.value.length}
          </b>
        </Button>
      </div>}
      <Notice message={error||mcp.error||skills.error||presets.error||catalogSkills.error} retrying={mcp.refreshing||skills.refreshing||presets.refreshing||catalogSkills.refreshing} onRetry={()=>{void Promise.all([mcp.reload(),skills.reload(),presets.reload(),catalogSkills.reload()]);if(tab==='market')void loadCatalog();if(tab==='installed')void discover(true)}}/>
      {message && (
        <div className="my-3 flex items-center gap-2 rounded-xl border border-success/20 bg-success-soft p-3 text-sm text-success">
          <Check size={16} />
          {message}
        </div>
      )}
      {tab === "market" && (
        <>
          <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
            <Subheading
              title={t('ext.registry')}
              extra={
                <div className="flex items-center gap-2">
                  <span className="text-[11px] text-muted">
                    {t('ext.realtime', { count: catalog.length })}
                  </span>
                  <Button
                    variant="secondary"
                    onClick={() => void loadCatalog(query.trim())}
                    disabled={searching}
                  >
                    <RefreshCw size={14} /> {t('ext.refresh')}
                  </Button>
                </div>
              }
            />
          </div>
          <Panel className="min-w-0 p-5">
            <div className="flex min-w-0 items-center gap-2 rounded-xl border border-divider bg-surface px-3 py-2">
              <Search size={18} />
              <TextInput
                value={query}
                placeholder={t('ext.searchPlaceholder')}
                onChange={(e) => setQuery(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && search()}
              />
              <Action onClick={search} disabled={searching}>
                {searching ? t('ext.searching') : t('ext.search')}
              </Action>
            </div>
            <div className="rounded-xl bg-canvas p-3 text-xs leading-5 text-muted">
              {t('ext.registryNote')}
            </div>
            <div className="registry-results">
              {catalog.map((x) => (
                <div
                  className="flex min-w-0 flex-wrap items-start gap-3 rounded-xl border border-divider bg-surface p-4 hover:border-divider"
                  key={x.name}
                >
                  <span className="grid size-10 shrink-0 place-items-center rounded-lg bg-brand-soft text-brand">
                    <Server size={19} />
                  </span>
                  <div className="min-w-0 flex-1">
                    <strong className="block break-words text-sm">{x.name}</strong>
                    {/[가-힣]/.test(x.description || '') ? (
                      <p className="mt-1 break-words text-xs leading-5 text-muted">{x.description}</p>
                    ) : (
                      <>
                        <p className="mt-1 break-words text-xs leading-5 text-muted">{koreanToolSummary(x.name, x.description)}</p>
                        {x.description && (
                          <details className="mt-1 text-[11px] text-muted">
                            <summary className="cursor-pointer select-none">원문 보기</summary>
                            <p className="mt-1 leading-5">{x.description}</p>
                          </details>
                        )}
                      </>
                    )}
                    <small className="mt-1 block break-all text-[11px] text-muted">{x.url || (x.packageName&&x.packageVersion?x.packageName+' · '+x.packageVersion:t('ext.noInstallInfo'))}</small>
                  </div>
                  {registryLaunch(x) &&
                    (mcp.value.some((r: any) => r.name === x.name) ? (
                      <StateTag online>{t('ext.registeredCheck')}</StateTag>
                    ) : (
                      <Button
                        variant="secondary"
                        onClick={() =>
                          install({name:x.name,...registryLaunch(x)})
                        }
                      >
                        {t('ext.install')}
                      </Button>
                    ))}
                </div>
              ))}
              {!catalog.length && !searching && (
                <Empty
                  icon={Compass}
                  title={t('ext.emptySearch')}
                  description={t('ext.emptySearchDesc')}
                />
              )}
            </div>
          </Panel>
          <details className="mt-5 rounded-xl border border-divider bg-surface">
            <summary className="cursor-pointer px-4 py-3 text-[13px] font-semibold text-ink">
              {loc==='en'?'Browse curated MCP server examples':'추천 MCP 서버 보기'} · {presets.value.length+featured.length}
            </summary>
            <div className="border-t border-divider p-4">
          <div className="mb-4 flex flex-wrap items-center gap-1.5">
            {pluginCategories.map((category) => (
              <Button
                key={category}
                variant={pluginCategory === category ? "primary" : "ghost"}
                onClick={() => setPluginCategory(category)}
                className="!px-3 !py-1.5 !text-[12px]"
              >
                {category === "all" ? t('ext.all') : category}
              </Button>
            ))}
          </div>
          <div className="mb-5 flex items-center gap-2 rounded-xl border border-divider bg-surface px-3 py-2">
            <Search size={17} className="text-muted" />
            <TextInput
              value={pluginQuery}
              onChange={(e) => setPluginQuery(e.target.value)}
              placeholder={t('ext.pluginSearchExample')}
            />
          </div>
          <Subheading
            title={loc==='en'?'Recommended MCP servers':'추천 MCP 서버'}
            extra={
              <span className="text-[11px] text-muted">
                {t('ext.count', { count: filteredPresets.length })}
              </span>
            }
          />
          <div className="mb-6 grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
            {filteredPresets.map((preset) => {
              const record = mcp.value.find((x) => x.name === preset.name);
              const connected = discoveryForMcp(tools,record?.id||'').tools.length;
              return (
                <Panel
                  key={preset.id}
                  className="flex min-w-0 flex-col gap-3 p-5"
                >
                  <div className="flex items-start justify-between gap-3">
                    <div className="grid size-11 shrink-0 place-items-center rounded-xl bg-brand-soft text-brand">
                      <PackagePlus size={21} />
                    </div>
                    <StateTag online={Boolean(record)}>
                      {record
                        ? connected
                          ? t('ext.connectedTools', { count: connected })
                          : t('ext.registeredCheck')
                        : t('ext.notInstalled')}
                    </StateTag>
                  </div>
                  <h3>{preset.name}</h3>
                  <p>{preset.description}</p>
                  <small className="text-[10px] font-semibold uppercase tracking-wider text-muted">
                    {preset.category}
                  </small>
                  <div className="mt-auto flex items-center justify-between gap-2 border-t border-divider pt-3">
                    <code className="truncate text-[10px] text-muted">
                      {preset.package}
                    </code>
                    {record ? (
                      <Button
                        variant="secondary"
                        className="shrink-0 whitespace-nowrap"
                        onClick={async () => {
                          try {
                            await del("/api/mcp/" + record.id);
                            await mcp.reload();
                            setMessage(t('ext.cancelled', { name: preset.name }));
                          } catch (e) {
                            setError((e as Error).message);
                          }
                        }}
                      >
                        {t('ext.cancel')}
                      </Button>
                    ) : (
                      <Button
                        variant="secondary"
                        className="shrink-0 whitespace-nowrap"
                        disabled={installing === preset.id}
                        onClick={() => void installPreset(preset)}
                      >
                        {installing === preset.id ? t('ext.installing') : t('ext.install')}
                      </Button>
                    )}
                  </div>
                </Panel>
              );
            })}
          </div>
          <Subheading
            title={loc==='en'?'Featured MCP connections':'주요 MCP 연결'}
            extra={
              <span className="text-[10px] font-bold uppercase tracking-widest text-muted">
                CURATED EXAMPLES
              </span>
            }
          />
          <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
            {featured.map((x) => {
              const Icon = x.icon;
              const record = mcp.value.find((y) => y.name === x.name);
              const connected = discoveryForMcp(tools,record?.id||'').tools.length;
              return (
                <Panel key={x.name} className="flex min-w-0 flex-col gap-3 p-5">
                  <div className="grid size-12 shrink-0 place-items-center rounded-xl bg-brand-soft text-brand">
                    <Icon size={24} />
                  </div>
                  <h3>{x.name}</h3>
                  <p>{loc === "en" ? x.descriptionEn || x.description : x.description}</p>
                  <div className="mt-auto flex items-center justify-between gap-3 border-t border-divider pt-3">
                    <a href={x.url} target="_blank" rel="noreferrer">
                      {t('ext.officialGuide')} <ExternalLink size={13} />
                    </a>
                    {record ? (
                      <Button
                        variant="secondary"
                        className="shrink-0 whitespace-nowrap"
                        onClick={async () => {
                          try {
                            await del("/api/mcp/" + record.id);
                            await mcp.reload();
                            setMessage(t('ext.cancelled', { name: x.name }));
                          } catch (e) {
                            setError((e as Error).message);
                          }
                        }}
                      >
                        {t('ext.cancel')}
                      </Button>
                    ) : (
                      <Button
                        variant="secondary"
                        className="shrink-0 whitespace-nowrap"
                        onClick={() => install(x)}
                      >
                        {t('ext.install')}
                      </Button>
                    )}
                  </div>
                </Panel>
              );
            })}
          </div>

            </div>
          </details>
        </>
      )}
      {tab === "plugins" && (
        <>
          <PluginDiscovery/>
          <details className="rounded-xl border border-divider p-4">
          <summary className="cursor-pointer text-sm font-semibold text-ink">{loc==='en'?'Local skill catalog':'로컬 스킬 카탈로그'}</summary>
          <div className="mt-4">
          <div className="mb-3 flex flex-wrap items-end justify-between gap-3">
            <div>
              <h3 className="m-0 text-[15px] font-bold text-ink">
                {t('ext.pluginSkillsTitle')}
              </h3>
              <p className="mt-1 text-[11px] text-muted">
                {t('ext.pluginSkillsDesc')}{" "}
                {t('ext.foundCount', { count: catalogSkills.value.total })} ·{" "}
                {catalogSkills.value.updatedAt
                  ? new Date(catalogSkills.value.updatedAt).toLocaleTimeString(loc === 'en' ? 'en-US' : 'ko-KR')
                  : t('ext.reading')}
              </p>
            </div>
            <Button
              variant="secondary"
              onClick={() => void catalogSkills.reload()}
            >
              <RefreshCw size={14} /> {t('ext.refresh')}
            </Button>
          </div>
          <div className="mb-3 flex flex-wrap items-center gap-1.5">
            {catalogSources.map((source) => (
              <Button
                key={source}
                variant={catalogSource === source ? "primary" : "ghost"}
                onClick={() => setCatalogSource(source)}
                className="!px-3 !py-1.5 !text-[12px]"
              >
                {source === "all" ? t('ext.allSources') : source}
              </Button>
            ))}
          </div>
          <div className="mb-3 grid gap-2 sm:grid-cols-[minmax(0,1fr)_220px]">
            <div className="flex items-center gap-2 rounded-xl border border-divider bg-surface px-3 py-2">
              <Search size={17} className="text-muted" />
              <TextInput
                value={catalogQuery}
                onChange={(e) => setCatalogQuery(e.target.value)}
                placeholder={t('ext.pluginSearch')}
              />
            </div>
            <SelectInput
              value={catalogCategory}
              onChange={(e) => setCatalogCategory(e.target.value)}
            >
              <option value="all">{t('ext.allCategories')}</option>
              {catalogCategories
                .filter((category) => category !== "all")
                .map((category) => (
                  <option key={category} value={category}>
                    {category}
                  </option>
                ))}
            </SelectInput>
          </div>
          <div className="mb-3 max-h-[440px] overflow-y-auto rounded-2xl border border-divider bg-surface">
            {visibleCatalog.map((item) => {
              const installed = skills.value.some(
                (skill) => skill.name === item.name,
              );
              return (
                <div
                  key={item.id}
                  className="flex flex-wrap items-center justify-between gap-3 border-b border-divider px-4 py-3 last:border-0"
                >
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <strong className="text-[12px] text-ink">
                        {item.name}
                      </strong>
                      <span className="rounded bg-canvas px-1.5 py-0.5 text-[10px] font-semibold text-muted">
                        {item.source}
                      </span>
                      <span className="text-[10px] text-muted">
                        {item.category}
                      </span>
                    </div>
                    <p className="mt-1 line-clamp-2 text-[11px] leading-5 text-muted">
                      {item.description}
                    </p>
                    {!item.available && (
                      <small className="mt-1 block text-[10px] text-warning">
                        {t('ext.requiresBinary', { list: (item.requires || []).join(', ') })}
                      </small>
                    )}
                  </div>
                  <div className="shrink-0">
                    {installed ? (
                      <Button
                        variant="secondary"
                        className="shrink-0 whitespace-nowrap"
                        onClick={async () => {
                          const target = skills.value.find(
                            (x) => x.name === item.name,
                          );
                          if (!target) return;
                          try {
                            await del("/api/skills/" + target.id);
                            await skills.reload();
                            setMessage(t('ext.cancelled', { name: item.name }));
                          } catch (e) {
                            setError((e as Error).message);
                          }
                        }}
                      >
                        {t('ext.cancel')}
                      </Button>
                    ) : !item.available ? (
                      <span className="rounded-md bg-warning-soft px-2 py-1 text-[10px] font-semibold text-warning">
                        {t('ext.requiredTool')}
                      </span>
                    ) : (
                      <Button
                        variant="secondary"
                        disabled={importing === item.id}
                        onClick={() => void importCatalogSkill(item)}
                      >
                        {importing === item.id ? t('ext.installing') : t('ext.install')}
                      </Button>
                    )}
                  </div>
                </div>
              );
            })}
            {!visibleCatalog.length && (
              <Empty
                icon={BookOpenText}
                title={t('ext.catalogEmpty')}
                description={t('ext.catalogHint')}
              />
            )}
          </div>
          <div className="mb-6 flex flex-wrap items-center justify-between gap-2 text-[11px] text-muted">
            <span>
              {t('ext.showing', { visible: visibleCatalog.length, total: filteredCatalog.length })}
            </span>
            {visibleCatalog.length < filteredCatalog.length && (
              <Button
                variant="secondary"
                onClick={() => setCatalogLimit((limit) => limit + 30)}
              >
                {t('ext.more')}
              </Button>
            )}
          </div>
          </div>
          </details>
        </>
      )}
      {tab === "installed" && (
        <>
          {!mcpOnlyEmbed&&<section className="mb-6 rounded-xl border border-divider bg-surface p-4">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div className="min-w-0">
                <h2 className="text-sm font-semibold text-ink">{loc==='en'?'Built-in tools':'기본 도구'}</h2>
                <p className="mt-1 text-xs leading-5 text-muted">{loc==='en'?'Search, files and code are built into DOMINION. MCP plugins below can be enabled independently.':'검색·파일·코드는 도미니온의 기본 도구입니다. 아래 MCP는 설치 후 개별적으로 끌 수 있습니다.'}</p>
              </div>
              <Button type="button" variant="secondary" size="sm" aria-expanded={showBuiltins} onClick={()=>setShowBuiltins(value=>!value)}>{showBuiltins?(loc==='en'?'Hide tools':'도구 목록 접기'):(loc==='en'?'View available tools':'도구 목록 보기')}</Button>
            </div>
            {showBuiltins&&<div className="mt-4 border-t border-divider pt-4"><CapabilitiesPage embedded navigate={()=>{}}/></div>}
          </section>}
          <Subheading
            title={t('ext.connectedServers')}
            extra={
              <div className="flex flex-wrap gap-2">
                <Action variant="secondary" onClick={() => {
                  setEdit({name: "", transport: "stdio", command: "npx", url: "", enabled: true});
                  setArgText("");
                }}><Plus size={16}/>{t('ext.addMcp')}</Action>
                <Action variant="secondary" onClick={() => void discover()}>
                  <Activity size={16}/>{t('ext.toolSearch')}
                </Action>
              </div>
            }
          />
          <div className="flex min-w-0 flex-col gap-3">
            {mcp.value.map((x) => (
              <Panel
                key={x.id}
                className="flex min-w-0 flex-wrap items-center justify-between gap-3 rounded-xl border border-divider p-4"
              >
                <div className="grid size-10 shrink-0 place-items-center rounded-xl bg-brand-soft text-brand">
                  <Blocks size={21} />
                </div>
                <div className="min-w-0 flex-1">
                  <strong className="block break-words text-sm">{x.name}</strong>
                  <p className="mt-1 break-all text-xs leading-5 text-muted">
                    {x.transport === "stdio" ? x.command + " " + (x.args || []).join(" ") : x.url}
                  </p>
                  <small className="mt-1 block text-[11px] text-muted">
                    {x.transport.toUpperCase()} · {x.enabled !== false ? t('conn.enable') : t('conn.status.disabled')}
                  </small>
                </div>
                <StateTag
                  online={x.enabled !== false && discoveryForMcp(tools,x.id).tools.length>0}
                >
                  {(() => {
                    const {tools:own,error:failure}=discoveryForMcp(tools,x.id);
                    if (x.enabled === false) return t('conn.status.disabled');
                    if (failure) return t('ext.connectionFailed');
                    if (own.length) return t('ext.connectedTools', { count: own.length });
                    return tools.length ? t('ext.checkConnection') : t('ext.checking');
                  })()}
                </StateTag>
                <label className="inline-flex shrink-0 items-center gap-2 rounded-lg border border-divider bg-surface px-3 py-2 text-xs text-ink">
                  <TextInput type="checkbox" className="size-4 accent-[var(--color-brand)]"
                    checked={x.enabled !== false} disabled={Boolean(mcpToggling) || mcpSaving}
                    aria-label={(loc === 'en' ? 'Enable MCP plugin ' : 'MCP 확장 기능 사용 ') + x.name}
                    onChange={event => void toggleMcp(x, event.target.checked)}/>
                  {mcpToggling===x.id?<LoaderCircle size={14} className="animate-spin" aria-label={t('models.saving')}/>:<span>{x.enabled !== false?(loc==='en'?'On':'사용 중'):(loc==='en'?'Off':'사용 안 함')}</span>}
                </label>
                <Button
                  size="icon"
                  variant="ghost"
                  title={t('ext.settings')}
                  aria-label={t('ext.settings') + ' ' + x.name}
                  onClick={() => {
                    setEdit(x);
                    setArgText((x.args || []).join("\n"));
                  }}
                >
                  <Settings2 size={17} />
                </Button>
                <Button
                  size="icon"
                  variant="ghost"
                  title={t('ext.remove')}
                  aria-label={t('ext.remove') + ' ' + x.name}
                  onClick={async () => {
                    if (confirm(t('ext.confirmRemove'))) {
                      try {
                        setError("");
                        await del("/api/mcp/" + x.id);
                        await mcp.reload();
                        setTools(previous => previous.filter(tool => tool.mcpId !== x.id));
                        setMessage(t('ext.cancelled', {name: x.name}));
                      } catch (e) {
                        setError((e as Error).message);
                      }
                    }
                  }}
                >
                  <Trash2 size={17} />
                </Button>
              </Panel>
            ))}
            {!mcp.value.length && (
              <Panel>
                <Empty
                  icon={Blocks}
                  title={t('ext.noServers')}
                  description={t('ext.noServersDesc')}
                />
              </Panel>
            )}
          </div>
          {tools.length > 0 && (
            <Panel className="grid gap-3">
              <Subheading
                title={t('ext.realTools')}
                extra={<span>{t('ext.count', { count: tools.filter((item) => !item.error).length })}</span>}
              />
              {tools.map((t) => (
                <div
                  key={t.name + t.mcpId}
                  className="overflow-auto rounded-xl border border-divider bg-canvas p-4 text-xs leading-6"
                >
                  <Code2 size={16} />
                  <strong>{t.name}</strong>
                  <span>{t.error || t.description}</span>
                </div>
              ))}
            </Panel>
          )}
        </>
      )}
      {tab === "skills" && (
        <SkillsPanel
          repo={repo} setRepo={setRepo} repoBusy={repoBusy} importRepo={importRepo}
          skills={skills} addSkillTemplate={addSkillTemplate} filteredSkills={filteredSkills}
          skillCategories={skillCategories} skillCategory={skillCategory} setSkillCategory={setSkillCategory}
          skillQuery={skillQuery} setSkillQuery={setSkillQuery} setSkillEdit={setSkillEdit}
          setError={setError} setMessage={setMessage}
        />
      )}
      {edit && (
        <DialogBackdrop onDismiss={() => setEdit(null)}>
          <DialogPanel size="wide">
            <div className="flex items-center justify-between border-b border-divider p-5">
              <div>
                <small>{t('ui.label.mcpConnection')}</small>
                <h2>{t('ext.register')}</h2>
              </div>
              <Button size="icon" variant="ghost" onClick={() => setEdit(null)}>
                <X size={18} />
              </Button>
            </div>
            <div className="grid gap-4 p-5">
              <FormField label={t('ext.displayName')}>
                <TextInput
                  value={edit.name}
                  onChange={(e) => setEdit({ ...edit, name: e.target.value })}
                />
              </FormField>
              <FormField label={t('ext.transport')}>
                <SelectInput
                  value={edit.transport}
                  onChange={(e) =>
                    setEdit({ ...edit, transport: e.target.value })
                  }
                >
                  <option value="stdio">{t('ext.stdio')}</option>
                  <option value="http">{t('ext.http')}</option>
                </SelectInput>
              </FormField>
              {edit.transport === "http" ? (
                <>
                  <FormField label={t('ext.url')}>
                    <TextInput
                      value={edit.url || ""}
                      onChange={(e) =>
                        setEdit({ ...edit, url: e.target.value })
                      }
                    />
                  </FormField>
                  <FormField label={t('ext.bearer')}>
                    <TextInput
                      type="password"
                      value={edit.token || ""}
                      onChange={(e) =>
                        setEdit({ ...edit, token: e.target.value })
                      }
                      placeholder={t('ext.optional')}
                    />
                  </FormField>
                </>
              ) : (
                <>
                  <FormField label={t('ext.executable')}>
                    <TextInput
                      value={edit.command || ""}
                      onChange={(e) =>
                        setEdit({ ...edit, command: e.target.value })
                      }
                    />
                  </FormField>
                  <FormField label={t('ext.args')}>
                    <TextArea
                      rows={4}
                      className="min-h-28"
                      value={argText}
                      onChange={(e) => setArgText(e.target.value)}
                    />
                  </FormField>
                </>
              )}
            </div>
            <div className="flex flex-wrap justify-end gap-2 border-t border-divider p-5">
              <Action variant="secondary" onClick={() => setEdit(null)}>
                {t('ext.cancel')}
              </Action>
              <Action onClick={saveMcp} disabled={mcpSaving || !edit.name?.trim()}>
                <Check size={16} /> {t('ext.save')}
              </Action>
            </div>
          </DialogPanel>
        </DialogBackdrop>
      )}
      {skillEdit && (
        <DialogBackdrop onDismiss={() => setSkillEdit(null)}>
          <DialogPanel size="wide">
            <div className="flex items-center justify-between border-b border-divider p-5">
              <div>
                <small>{t('ui.label.skillEditor')}</small>
                <h2>{t('ext.skillWrite')}</h2>
              </div>
              <Button
                size="icon"
                variant="ghost"
                onClick={() => setSkillEdit(null)}
              >
                <X size={18} />
              </Button>
            </div>
            <div className="grid gap-4 p-5">
              <FormField label={t('ext.skillName')}>
                <TextInput
                  value={skillEdit.name || ""}
                  onChange={(e) =>
                    setSkillEdit({ ...skillEdit, name: e.target.value })
                  }
                />
              </FormField>
              <FormField label={t('ext.skillInstructions')}>
                <TextArea
                  className="min-h-28"
                  rows={10}
                  value={skillEdit.instructions || ""}
                  onChange={(e) =>
                    setSkillEdit({ ...skillEdit, instructions: e.target.value })
                  }
                />
              </FormField>
              <label className="inline-flex items-center gap-2 text-sm text-ink">
                <TextInput
                  type="checkbox"
                  checked={Boolean(skillEdit.enabled)}
                  onChange={(e) =>
                    setSkillEdit({
                      ...skillEdit,
                      enabled: e.target.checked,
                      status: e.target.checked ? "approved" : "proposed",
                    })
                  }
                />{" "}
                {t('ext.applySystem')}
              </label>
            </div>
            <div className="flex flex-wrap justify-end gap-2 border-t border-divider p-5">
              <Action variant="secondary" onClick={() => setSkillEdit(null)}>
                {t('ext.cancel')}
              </Action>
              <Action onClick={saveSkill}>
                <Check size={16} /> {t('ext.saveSkill')}
              </Action>
            </div>
          </DialogPanel>
        </DialogBackdrop>
      )}
    </div>
  );
}
