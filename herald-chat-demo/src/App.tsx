import { useState, useEffect, useRef } from "react";
import { MockHeraldClient } from "./mock/herald-mock";
import { HeraldContext } from "./hooks/useHerald";
import { Sidebar } from "./components/Sidebar";
import { ChatView } from "./components/ChatView";
import { streams } from "./data/seed";
import "./App.css";

export function App() {
  const clientRef = useRef<MockHeraldClient | null>(null);
  const [ready, setReady] = useState(false);
  const [activeStream, setActiveStream] = useState(streams[0].id);

  useEffect(() => {
    const client = new MockHeraldClient();
    clientRef.current = client;

    client.connect().then(() => {
      // Subscribe to all streams (mirrors real Herald usage)
      return client.subscribe(streams.map((s) => s.id));
    }).then(() => {
      setReady(true);
    });

    return () => {
      client.disconnect();
    };
  }, []);

  if (!ready || !clientRef.current) {
    return (
      <div className="loading">
        <div className="loading-spinner" />
        <p>Connecting to Herald...</p>
      </div>
    );
  }

  return (
    <HeraldContext.Provider value={clientRef.current}>
      <div className="app">
        <Sidebar activeStream={activeStream} onSelectStream={setActiveStream} />
        <ChatView key={activeStream} streamId={activeStream} />
      </div>
    </HeraldContext.Provider>
  );
}
