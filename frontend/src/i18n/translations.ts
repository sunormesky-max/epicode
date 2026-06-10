export type Language = 'zh' | 'en';

// ── Phase 1 Translation Keys ──
export type TranslationKey =
  // Common
  | 'common.loading' | 'common.error' | 'common.retry' | 'common.save' | 'common.cancel'
  | 'common.delete' | 'common.edit' | 'common.create' | 'common.search' | 'common.copy'
  | 'common.copied' | 'common.close' | 'common.submit' | 'common.confirm' | 'common.back'
  | 'common.next' | 'common.prev' | 'common.of'
  // Navigation
  | 'nav.home' | 'nav.quickStart' | 'nav.docs' | 'nav.community' | 'nav.benchmarks'
  | 'nav.console' | 'nav.getStarted' | 'nav.overview' | 'nav.memories' | 'nav.graph'
  | 'nav.skills' | 'nav.subAccounts' | 'nav.logout'
  // Footer
  | 'footer.brand' | 'footer.tagline' | 'footer.docs' | 'footer.quickStart' | 'footer.apiDocs'
  | 'footer.mcpProtocol' | 'footer.benchmarks' | 'footer.community' | 'footer.communitySkills'
  | 'footer.github' | 'footer.discord' | 'footer.copyright' | 'footer.version'
  // Home - Hero
  | 'home.hero.overline' | 'home.hero.title' | 'home.hero.taglineZh' | 'home.hero.taglineEn'
  | 'home.hero.ctaPrimary' | 'home.hero.ctaSecondary'
  | 'home.hero.stat1Num' | 'home.hero.stat1Label'
  | 'home.hero.stat2Num' | 'home.hero.stat2Label'
  | 'home.hero.stat3Num' | 'home.hero.stat3Label'
  // Home - Marquee
  | 'home.marquee.1' | 'home.marquee.2' | 'home.marquee.3' | 'home.marquee.4'
  | 'home.marquee.5' | 'home.marquee.6' | 'home.marquee.7'
  // Home - Features
  | 'home.features.overline' | 'home.features.title' | 'home.features.subtitle'
  | 'home.feature1.num' | 'home.feature1.title' | 'home.feature1.desc' | 'home.feature1.link'
  | 'home.feature2.num' | 'home.feature2.title' | 'home.feature2.desc' | 'home.feature2.link'
  | 'home.feature3.num' | 'home.feature3.title' | 'home.feature3.desc' | 'home.feature3.link'
  | 'home.feature4.num' | 'home.feature4.title' | 'home.feature4.desc' | 'home.feature4.link'
  // Home - QuickStart
  | 'home.quickStart.overline' | 'home.quickStart.title' | 'home.quickStart.desc'
  | 'home.quickStart.feat1' | 'home.quickStart.feat2' | 'home.quickStart.feat3'
  // Home - SystemSkills
  | 'home.skills.overline' | 'home.skills.title' | 'home.skills.subtitle'
  | 'home.skill1.name' | 'home.skill1.desc'
  | 'home.skill2.name' | 'home.skill2.desc'
  | 'home.skill3.name' | 'home.skill3.desc'
  | 'home.skill4.name' | 'home.skill4.desc'
  | 'home.skill5.name' | 'home.skill5.desc'
  | 'home.skill6.name' | 'home.skill6.desc'
  | 'home.skill7.name' | 'home.skill7.desc'
  | 'home.skill8.name' | 'home.skill8.desc'
  // Home - API Endpoints
  | 'home.api.overline' | 'home.api.title' | 'home.api.subtitle'
  | 'home.api.ep1method' | 'home.api.ep1path' | 'home.api.ep1desc'
  | 'home.api.ep2method' | 'home.api.ep2path' | 'home.api.ep2desc'
  | 'home.api.ep3method' | 'home.api.ep3path' | 'home.api.ep3desc'
  | 'home.api.ep4method' | 'home.api.ep4path' | 'home.api.ep4desc'
  | 'home.api.ep5method' | 'home.api.ep5path' | 'home.api.ep5desc'
  | 'home.api.ep6method' | 'home.api.ep6path' | 'home.api.ep6desc'
  | 'home.api.ep7method' | 'home.api.ep7path' | 'home.api.ep7desc'
  | 'home.api.ep8method' | 'home.api.ep8path' | 'home.api.ep8desc'
  // Home - CTA
  | 'home.cta.title' | 'home.cta.subtitle' | 'home.cta.button' | 'home.cta.note'
  // Login
  | 'login.title' | 'login.subtitle' | 'login.username' | 'login.password'
  | 'login.submit' | 'login.loading' | 'login.registerLink' | 'login.error'
  | 'login.success'
  // Register
  | 'register.title' | 'register.subtitle' | 'register.username'
  | 'register.password' | 'register.confirmPassword' | 'register.inviteCode'
  | 'register.submit' | 'register.loginLink' | 'register.passwordMismatch'
  | 'register.passwordWeak' | 'register.passwordFair' | 'register.passwordGood'
  | 'register.passwordStrong' | 'register.success'
  // Dashboard (partial for Phase 1)
  | 'dash.totalMemories' | 'dash.thisWeek' | 'dash.activeClusters'
  | 'dash.energy' | 'dash.avgQuery' | 'dash.apiCalls'
  | 'dash.quickStore' | 'dash.quickSearch' | 'dash.quickDigest' | 'dash.quickDocs'
  ;

export const translations: Record<Language, Record<TranslationKey, string>> = {
  zh: {
    // Common
    'common.loading': '加载中...',
    'common.error': '出错了',
    'common.retry': '重试',
    'common.save': '保存',
    'common.cancel': '取消',
    'common.delete': '删除',
    'common.edit': '编辑',
    'common.create': '创建',
    'common.search': '搜索',
    'common.copy': '复制',
    'common.copied': '已复制!',
    'common.close': '关闭',
    'common.submit': '提交',
    'common.confirm': '确认',
    'common.back': '返回',
    'common.next': '下一页',
    'common.prev': '上一页',
    'common.of': '/',
    // Navigation
    'nav.home': '首页',
    'nav.quickStart': '快速上手',
    'nav.docs': '文档',
    'nav.community': '社区',
    'nav.benchmarks': '基准测试',
    'nav.console': '控制台',
    'nav.getStarted': '开始使用',
    'nav.overview': '总览',
    'nav.memories': '记忆',
    'nav.graph': '知识图谱',
    'nav.skills': '技能',
    'nav.subAccounts': '子账户',
    'nav.logout': '退出登录',
    // Footer
    'footer.brand': 'Epicode — AI Memory Operating System',
    'footer.tagline': '给 AI 一个永不遗忘的记忆',
    'footer.docs': '文档',
    'footer.quickStart': '快速上手',
    'footer.apiDocs': 'API 文档',
    'footer.mcpProtocol': 'MCP 协议',
    'footer.benchmarks': '性能基准',
    'footer.community': '社区',
    'footer.communitySkills': '社区技能',
    'footer.github': 'GitHub',
    'footer.discord': 'Discord',
    'footer.copyright': '© 2025 Epicode. 保留所有权利。',
    'footer.version': 'v1.0.0',
    // Home - Hero
    'home.hero.overline': 'AI 记忆操作系统',
    'home.hero.title': 'Epicode',
    'home.hero.taglineZh': '给 AI 一个永不遗忘的记忆',
    'home.hero.taglineEn': 'Give AI an unforgettable memory',
    'home.hero.ctaPrimary': '开始使用',
    'home.hero.ctaSecondary': '查看文档',
    'home.hero.stat1Num': '',
    'home.hero.stat1Label': '注册用户',
    'home.hero.stat2Num': '',
    'home.hero.stat2Label': '记忆存储量',
    'home.hero.stat3Num': '27',
    'home.hero.stat3Label': 'MCP 工具',
    // Marquee
    'home.marquee.1': '向量记忆',
    'home.marquee.2': '语义搜索',
    'home.marquee.3': '知识图谱',
    'home.marquee.4': 'MCP 集成',
    'home.marquee.5': '跨端点',
    'home.marquee.6': '生命周期',
    'home.marquee.7': 'API Key 认证',
    // Features
    'home.features.overline': '核心能力',
    'home.features.title': '四大记忆支柱',
    'home.features.subtitle': 'Epicode 为 AI 长期记忆提供基础基础设施',
    'home.feature1.num': '01',
    'home.feature1.title': '向量记忆存储',
    'home.feature1.desc': '为 AI 记忆提供持久化向量存储。每一条信息都被嵌入、索引，并在跨会话中可检索。',
    'home.feature1.link': '了解更多 →',
    'home.feature2.num': '02',
    'home.feature2.title': '语义搜索',
    'home.feature2.desc': '通过含义而不仅是关键词查找记忆。自然语言查询返回上下文相关的结果及相似度评分。',
    'home.feature2.link': '了解更多 →',
    'home.feature3.num': '03',
    'home.feature3.title': '知识图谱',
    'home.feature3.desc': '自动关系提取创建互联记忆的动态图谱，实现深度回忆和上下文理解。',
    'home.feature3.link': '了解更多 →',
    'home.feature4.num': '04',
    'home.feature4.title': 'MCP 集成',
    'home.feature4.desc': '27 个 MCP 工具提供对记忆操作的统一访问。标准化协议让任何 AI 代理都能存储、搜索和回忆。',
    'home.feature4.link': '了解更多 →',
    // QuickStart
    'home.quickStart.overline': '快速上手',
    'home.quickStart.title': '几分钟内开始',
    'home.quickStart.desc': '只需几行代码即可将 Epicode 集成到您的 AI 代理中。',
    'home.quickStart.feat1': '异步支持，自动重试',
    'home.quickStart.feat2': '完整的 TypeScript 类型安全',
    'home.quickStart.feat3': '零配置即可使用',
    // SystemSkills
    'home.skills.overline': '内置智能',
    'home.skills.title': '八大系统技能',
    'home.skills.subtitle': '开箱即用的预配置能力',
    'home.skill1.name': '记忆智能存取',
    'home.skill1.desc': 'Smart Memory Access',
    'home.skill2.name': '自动进化循环',
    'home.skill2.desc': 'Auto Evolution',
    'home.skill3.name': '技能发现引擎',
    'home.skill3.desc': 'Skill Discovery',
    'home.skill4.name': '知识图谱导航',
    'home.skill4.desc': 'Graph Navigation',
    'home.skill5.name': '上下文管理',
    'home.skill5.desc': 'Context Management',
    'home.skill6.name': '质量自控',
    'home.skill6.desc': 'Quality Control',
    'home.skill7.name': '系统全览',
    'home.skill7.desc': 'System Overview',
    'home.skill8.name': '对话智能',
    'home.skill8.desc': 'Dialogue Intelligence',
    // API Endpoints
    'home.api.overline': 'API 参考',
    'home.api.title': 'RESTful API 端点',
    'home.api.subtitle': '全面的记忆操作接口',
    'home.api.ep1method': 'GET',
    'home.api.ep1path': '/health',
    'home.api.ep1desc': '健康检查',
    'home.api.ep2method': 'POST',
    'home.api.ep2path': '/register',
    'home.api.ep2desc': '用户注册',
    'home.api.ep3method': 'POST',
    'home.api.ep3path': '/v1/login',
    'home.api.ep3desc': '用户登录',
    'home.api.ep4method': 'POST',
    'home.api.ep4path': '/v1/remember',
    'home.api.ep4desc': '存储记忆',
    'home.api.ep5method': 'POST',
    'home.api.ep5path': '/v1/search',
    'home.api.ep5desc': '语义搜索',
    'home.api.ep6method': 'GET',
    'home.api.ep6path': '/v1/stats',
    'home.api.ep6desc': '用户统计',
    'home.api.ep7method': 'GET',
    'home.api.ep7path': '/v1/timeline',
    'home.api.ep7desc': '记忆时间线',
    'home.api.ep8method': 'POST',
    'home.api.ep8path': '/mcp',
    'home.api.ep8desc': 'MCP 统一入口',
    // CTA
    'home.cta.title': '准备好赋予 AI 记忆了吗？',
    'home.cta.subtitle': '立即开始构建持久化的 AI 体验。',
    'home.cta.button': '免费开始使用',
    'home.cta.note': '无需信用卡。免费版包含 1,000 条记忆。',
    // Login
    'login.title': '登录',
    'login.subtitle': '输入凭证以访问控制台',
    'login.username': '用户名',
    'login.password': '密码',
    'login.submit': '登录',
    'login.loading': '登录中...',
    'login.registerLink': '还没有账号？注册',
    'login.error': '用户名或密码错误',
    'login.success': '登录成功',
    // Register
    'register.title': '创建账号',
    'register.subtitle': '注册获取 Epicode API 访问权限',
    'register.username': '选择用户名',
    'register.password': '创建密码',
    'register.confirmPassword': '确认密码',
    'register.inviteCode': '邀请码（可选）',
    'register.submit': '创建账号',
    'register.loginLink': '已有账号？登录',
    'register.passwordMismatch': '两次输入的密码不一致',
    'register.passwordWeak': '弱',
    'register.passwordFair': '一般',
    'register.passwordGood': '良好',
    'register.passwordStrong': '强',
    'register.success': '注册成功',
    // Dashboard
    'dash.totalMemories': '记忆总数',
    'dash.thisWeek': '本周新增',
    'dash.activeClusters': '活跃簇数',
    'dash.energy': '能量',
    'dash.avgQuery': '平均查询时间',
    'dash.apiCalls': 'API 调用',
    'dash.quickStore': '存储记忆',
    'dash.quickSearch': '搜索记忆',
    'dash.quickDigest': '消化文件',
    'dash.quickDocs': '查看文档',
  },
  en: {
    // Common
    'common.loading': 'Loading...',
    'common.error': 'Error',
    'common.retry': 'Retry',
    'common.save': 'Save',
    'common.cancel': 'Cancel',
    'common.delete': 'Delete',
    'common.edit': 'Edit',
    'common.create': 'Create',
    'common.search': 'Search',
    'common.copy': 'Copy',
    'common.copied': 'Copied!',
    'common.close': 'Close',
    'common.submit': 'Submit',
    'common.confirm': 'Confirm',
    'common.back': 'Back',
    'common.next': 'Next',
    'common.prev': 'Previous',
    'common.of': 'of',
    // Navigation
    'nav.home': 'Home',
    'nav.quickStart': 'Quick Start',
    'nav.docs': 'Docs',
    'nav.community': 'Community',
    'nav.benchmarks': 'Benchmarks',
    'nav.console': 'Console',
    'nav.getStarted': 'Get Started',
    'nav.overview': 'Overview',
    'nav.memories': 'Memories',
    'nav.graph': 'Knowledge Graph',
    'nav.skills': 'Skills',
    'nav.subAccounts': 'Sub Accounts',
    'nav.logout': 'Logout',
    // Footer
    'footer.brand': 'Epicode — AI Memory Operating System',
    'footer.tagline': 'Give AI an unforgettable memory',
    'footer.docs': 'Documentation',
    'footer.quickStart': 'Quick Start',
    'footer.apiDocs': 'API Docs',
    'footer.mcpProtocol': 'MCP Protocol',
    'footer.benchmarks': 'Benchmarks',
    'footer.community': 'Community',
    'footer.communitySkills': 'Community Skills',
    'footer.github': 'GitHub',
    'footer.discord': 'Discord',
    'footer.copyright': '© 2025 Epicode. All rights reserved.',
    'footer.version': 'v1.0.0',
    // Home - Hero
    'home.hero.overline': 'AI MEMORY OPERATING SYSTEM',
    'home.hero.title': 'Epicode',
    'home.hero.taglineZh': '给 AI 一个永不遗忘的记忆',
    'home.hero.taglineEn': 'Give AI an unforgettable memory',
    'home.hero.ctaPrimary': 'Get Started',
    'home.hero.ctaSecondary': 'View Documentation',
    'home.hero.stat1Num': '',
    'home.hero.stat1Label': 'Registered Users',
    'home.hero.stat2Num': '',
    'home.hero.stat2Label': 'Memories Stored',
    'home.hero.stat3Num': '27',
    'home.hero.stat3Label': 'MCP Tools',
    // Marquee
    'home.marquee.1': 'Vector Memory',
    'home.marquee.2': 'Semantic Search',
    'home.marquee.3': 'Knowledge Graph',
    'home.marquee.4': 'MCP Integration',
    'home.marquee.5': 'Cross-Endpoint',
    'home.marquee.6': 'Lifecycle',
    'home.marquee.7': 'API Key Auth',
    // Features
    'home.features.overline': 'CORE CAPABILITIES',
    'home.features.title': 'Four Pillars of Memory',
    'home.features.subtitle': 'Epicode provides foundational infrastructure for AI long-term memory',
    'home.feature1.num': '01',
    'home.feature1.title': 'Vector Memory Store',
    'home.feature1.desc': 'Persistent vector storage for AI memories. Every piece of information is embedded, indexed, and retrievable across sessions.',
    'home.feature1.link': 'Learn more →',
    'home.feature2.num': '02',
    'home.feature2.title': 'Semantic Search',
    'home.feature2.desc': 'Find memories by meaning, not just keywords. Natural language queries return contextually relevant results with similarity scores.',
    'home.feature2.link': 'Learn more →',
    'home.feature3.num': '03',
    'home.feature3.title': 'Knowledge Graph',
    'home.feature3.desc': 'Automatic relationship extraction creates a living graph of interconnected memories, enabling deep recall and contextual understanding.',
    'home.feature3.link': 'Learn more →',
    'home.feature4.num': '04',
    'home.feature4.title': 'MCP Integration',
    'home.feature4.desc': '27 MCP tools provide unified access to memory operations. Standardized protocol for any AI agent to store, search, and recall.',
    'home.feature4.link': 'Learn more →',
    // QuickStart
    'home.quickStart.overline': 'QUICK START',
    'home.quickStart.title': 'Start in Minutes',
    'home.quickStart.desc': 'Integrate Epicode into your AI agent with just a few lines of code.',
    'home.quickStart.feat1': 'Async support with automatic retry',
    'home.quickStart.feat2': 'Type-safe client with full TypeScript',
    'home.quickStart.feat3': 'Zero configuration required',
    // SystemSkills
    'home.skills.overline': 'BUILT-IN INTELLIGENCE',
    'home.skills.title': 'Eight System Skills',
    'home.skills.subtitle': 'Pre-configured capabilities that work out of the box',
    'home.skill1.name': 'Smart Memory Access',
    'home.skill1.desc': '记忆智能存取',
    'home.skill2.name': 'Auto Evolution',
    'home.skill2.desc': '自动进化循环',
    'home.skill3.name': 'Skill Discovery',
    'home.skill3.desc': '技能发现引擎',
    'home.skill4.name': 'Graph Navigation',
    'home.skill4.desc': '知识图谱导航',
    'home.skill5.name': 'Context Management',
    'home.skill5.desc': '上下文管理',
    'home.skill6.name': 'Quality Control',
    'home.skill6.desc': '质量自控',
    'home.skill7.name': 'System Overview',
    'home.skill7.desc': '系统全览',
    'home.skill8.name': 'Dialogue Intelligence',
    'home.skill8.desc': '对话智能',
    // API Endpoints
    'home.api.overline': 'API REFERENCE',
    'home.api.title': 'RESTful API Endpoints',
    'home.api.subtitle': 'Comprehensive endpoints for memory operations',
    'home.api.ep1method': 'GET',
    'home.api.ep1path': '/health',
    'home.api.ep1desc': 'Health check',
    'home.api.ep2method': 'POST',
    'home.api.ep2path': '/register',
    'home.api.ep2desc': 'User registration',
    'home.api.ep3method': 'POST',
    'home.api.ep3path': '/v1/login',
    'home.api.ep3desc': 'User login',
    'home.api.ep4method': 'POST',
    'home.api.ep4path': '/v1/remember',
    'home.api.ep4desc': 'Store memory',
    'home.api.ep5method': 'POST',
    'home.api.ep5path': '/v1/search',
    'home.api.ep5desc': 'Semantic search',
    'home.api.ep6method': 'GET',
    'home.api.ep6path': '/v1/stats',
    'home.api.ep6desc': 'User statistics',
    'home.api.ep7method': 'GET',
    'home.api.ep7path': '/v1/timeline',
    'home.api.ep7desc': 'Memory timeline',
    'home.api.ep8method': 'POST',
    'home.api.ep8path': '/mcp',
    'home.api.ep8desc': 'MCP unified entry',
    // CTA
    'home.cta.title': 'Ready to Give Your AI Memory?',
    'home.cta.subtitle': 'Start building persistent AI experiences today.',
    'home.cta.button': 'Get Started Free',
    'home.cta.note': 'No credit card required. Free tier includes 1,000 memories.',
    // Login
    'login.title': 'Sign In',
    'login.subtitle': 'Enter your credentials to access the console',
    'login.username': 'Username',
    'login.password': 'Password',
    'login.submit': 'Sign In',
    'login.loading': 'Signing in...',
    'login.registerLink': "Don't have an account? Register",
    'login.error': 'Invalid username or password',
    'login.success': 'Login successful',
    // Register
    'register.title': 'Create Account',
    'register.subtitle': 'Register for Epicode API access',
    'register.username': 'Choose a username',
    'register.password': 'Create password',
    'register.confirmPassword': 'Confirm password',
    'register.inviteCode': 'Invite code (optional)',
    'register.submit': 'Create Account',
    'register.loginLink': 'Already have an account? Sign In',
    'register.passwordMismatch': 'Passwords do not match',
    'register.passwordWeak': 'Weak',
    'register.passwordFair': 'Fair',
    'register.passwordGood': 'Good',
    'register.passwordStrong': 'Strong',
    'register.success': 'Account created successfully',
    // Dashboard
    'dash.totalMemories': 'Total Memories',
    'dash.thisWeek': 'This Week',
    'dash.activeClusters': 'Active Clusters',
    'dash.energy': 'Energy',
    'dash.avgQuery': 'Avg Query Time',
    'dash.apiCalls': 'API Calls',
    'dash.quickStore': 'Store Memory',
    'dash.quickSearch': 'Search Memory',
    'dash.quickDigest': 'Digest File',
    'dash.quickDocs': 'View Docs',
  },
};
