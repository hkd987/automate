import { HashRouter } from 'react-router-dom'
import { AppRouter } from './router'
import { Layout } from './components/Layout'

function App() {
  return (
    <HashRouter>
      <Layout>
        <AppRouter />
      </Layout>
    </HashRouter>
  )
}

export default App
