import { useState } from 'react';
import { SearchBar } from './SearchBar';
import { SearchResults } from './SearchResults';

interface SearchProps {
  sessionId?: string;
}

export function Search({ sessionId }: SearchProps) {
  const [query, setQuery] = useState('');

  return (
    <div className="container mx-auto px-4 py-8">
      <div className="mb-8">
        <h1 className="text-3xl font-bold text-gray-900 mb-2">Search OCR Results</h1>
        <p className="text-gray-600">
          Search through all text extracted from your screen recordings
        </p>
      </div>

      <div className="mb-8 flex justify-center">
        <SearchBar onSearch={setQuery} />
      </div>

      <SearchResults query={query} sessionId={sessionId} />
    </div>
  );
}
