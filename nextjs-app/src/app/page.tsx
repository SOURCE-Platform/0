'use client' 

import { useEffect, useState } from 'react'
import { supabase } from '@/lib/supabaseClient' 

type Note = {
  id: number;
  content: string;
  created_at: string;
}

export default function Home() {
  const [notes, setNotes] = useState<Note[] | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const fetchNotes = async () => {
      setLoading(true);
      setError(null);
      try {
        const { data, error } = await supabase.from('notes').select('*');
        if (error) throw error;
        setNotes(data);
      } catch (err: any) {
        console.error("Error fetching notes:", err);
        setError(err.message || "Failed to fetch notes.");
      } finally {
        setLoading(false); 
      }
    };
    fetchNotes();
  }, []); 

  return (
    <main className="flex min-h-screen flex-col items-center justify-p24 p-6">
      <h1 className="text-4xl font-bold">Welcome to Project O!</h1>
      <p className="mt-4">Phase 1 Setup</p>
      <div className="mt-8 border-t pt-4">
        <h2 className="text-2xl mb-2">Supabase Test Data:</h2>
        {loading && <p>Loading...</p>}
        {error && <p className="text-red-500">Error: {error}</p>}
        {notes && notes.length > 0 && (
          <ul>
            {notes.map((note) => (
              <li key={note.id} className="border-b py-1">
                {note.content} (ID: {note.id})
              </li>
            ))}
          </ul>
        )}
        {notes && notes.length === 0 && (
          <p>No notes found. Please create a test note in Supabase.</p>
        )}
      </div>
    </main>
  );
}
