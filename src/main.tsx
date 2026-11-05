import { Component, StrictMode, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import './styles.css'
import App from './App.tsx'
import { dbg } from './dbg' // 崩溃诊断日志：全局未捕获异常写本地 vct-debug.log

// ── 顶层 error boundary（黑屏根因修复配套防线） ──
// 渲染层再抛错时保住界面（唔再整窗黑屏）：显示错误摘要＋重新加载入口，
// 错误全文写本地日志（vct-debug.log）。
class TopBoundary extends Component<{ children: ReactNode }, { err: Error | null }> {
  state = { err: null as Error | null }

  static getDerivedStateFromError(err: Error) {
    return { err }
  }

  componentDidCatch(err: Error, info: React.ErrorInfo) {
    dbg(
      `[boundary] ${err.message} | ${err.stack ?? ''} | ${info.componentStack ?? ''}`.slice(0, 4000),
    )
  }

  render() {
    if (this.state.err) {
      return (
        <div
          style={{
            minHeight: '100vh',
            display: 'flex',
            flexDirection: 'column',
            gap: 12,
            padding: 24,
            background: 'var(--bg)',
            color: 'var(--text)',
            font: '14px/1.6 -apple-system, "PingFang SC", sans-serif',
          }}
        >
          <strong>界面渲染出错（已保住窗口，不再整窗黑屏）</strong>
          <pre
            style={{
              whiteSpace: 'pre-wrap',
              color: 'light-dark(#b3261e, #ff8080)',
              maxHeight: 200,
              overflow: 'auto',
              margin: 0,
            }}
          >
            {String(this.state.err.message || this.state.err)}
          </pre>
          <div style={{ display: 'flex', gap: 8 }}>
            <button
              className="btn primary"
              onClick={() => {
                window.location.reload()
              }}
            >
              重新加载窗口
            </button>
          </div>
          <span style={{ color: 'var(--text-2)' }}>详细错误已写入本地日志 vct-debug.log</span>
        </div>
      )
    }
    return this.props.children
  }
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <TopBoundary>
      <App />
    </TopBoundary>
  </StrictMode>,
)
