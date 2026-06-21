# React State Management with Zustand

## 描述
在 React 项目中使用 Zustand 实现简洁、高效的状态管理，避免 Redux 的样板代码。

## 触发条件
- 需要全局状态管理但不想使用 Redux
- 需要轻量级的状态共享方案
- 需要 TypeScript 友好的状态管理

## 步骤

### 1. 安装 Zustand

```bash
npm install zustand
```

### 2. 创建 Store

```typescript
import { create } from 'zustand';
import { persist } from 'zustand/middleware';

interface UserState {
  user: User | null;
  isAuthenticated: boolean;
  login: (user: User) => void;
  logout: () => void;
  updateProfile: (updates: Partial<User>) => void;
}

export const useUserStore = create<UserState>()(
  persist(
    (set) => ({
      user: null,
      isAuthenticated: false,
      login: (user) => set({ user, isAuthenticated: true }),
      logout: () => set({ user: null, isAuthenticated: false }),
      updateProfile: (updates) =>
        set((state) => ({
          user: state.user ? { ...state.user, ...updates } : null,
        })),
    }),
    {
      name: 'user-storage',
    }
  )
);
```

### 3. 在组件中使用

```typescript
import { useUserStore } from './store';

function UserProfile() {
  const { user, updateProfile } = useUserStore();
  
  const handleUpdate = () => {
    updateProfile({ name: '新名字' });
  };
  
  return (
    <div>
      <p>{user?.name}</p>
      <button onClick={handleUpdate}>更新</button>
    </div>
  );
}
```

### 4. 选择器优化

```typescript
// 只订阅需要的字段，避免不必要的重渲染
const userName = useUserStore((state) => state.user?.name);
const login = useUserStore((state) => state.login);
```

## 示例

### 多 Store 组合

```typescript
// authStore.ts
export const useAuthStore = create<AuthState>((set) => ({
  token: null,
  setToken: (token) => set({ token }),
}));

// userStore.ts  
export const useUserStore = create<UserState>((set) => ({
  profile: null,
  fetchProfile: async () => {
    const token = useAuthStore.getState().token;
    const profile = await api.getProfile(token);
    set({ profile });
  },
}));
```

## 标签

- category: react
- difficulty: beginner
- language: typescript
- author: 冬儿
- created: 2026-06-21

## 参考

- [Zustand 文档](https://docs.pmnd.rs/zustand)
- [React 状态管理对比](https://github.com/sunormesky-max/epicode/discussions)
