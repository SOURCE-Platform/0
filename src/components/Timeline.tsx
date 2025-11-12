import { useRef, useEffect } from 'react';
import * as d3 from 'd3';
import { format } from 'date-fns';
import { TimelineData, TimelineSession, TimelineZoom, SessionType } from '../types/timeline';

interface TimelineProps {
  data: TimelineData;
  zoom: TimelineZoom;
  onTimeClick: (timestamp: number) => void;
  onSessionClick: (sessionId: string) => void;
}

export function Timeline({ data, zoom, onTimeClick, onSessionClick }: TimelineProps) {
  const svgRef = useRef<SVGSVGElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!svgRef.current || !data.sessions.length || !containerRef.current) return;
    renderTimeline();
  }, [data, zoom]);

  const renderTimeline = () => {
    if (!svgRef.current || !containerRef.current) return;

    const svg = d3.select(svgRef.current);
    svg.selectAll('*').remove();

    const width = containerRef.current.clientWidth;
    const height = 400;
    const margin = { top: 20, right: 20, bottom: 60, left: 60 };

    // Create scales
    const xScale = d3.scaleTime()
      .domain([data.dateRange.start, data.dateRange.end])
      .range([margin.left, width - margin.right]);

    const yScale = d3.scaleBand()
      .domain(data.sessions.map(s => s.id))
      .range([margin.top, height - margin.bottom])
      .padding(0.2);

    // Draw X axis
    const xAxis = d3.axisBottom(xScale)
      .ticks(getTickCount(zoom))
      .tickFormat((d) => getTickFormat(d as Date, zoom));

    svg.append('g')
      .attr('transform', `translate(0, ${height - margin.bottom})`)
      .call(xAxis)
      .selectAll('text')
      .attr('transform', 'rotate(-45)')
      .style('text-anchor', 'end');

    // Draw session blocks
    const sessions = svg.selectAll('.session')
      .data(data.sessions)
      .enter()
      .append('g')
      .attr('class', 'session')
      .attr('transform', (d) => `translate(0, ${yScale(d.id)})`);

    // Session background
    sessions.append('rect')
      .attr('x', (d) => xScale(d.startTimestamp))
      .attr('width', (d) => {
        const end = d.endTimestamp || Date.now();
        return xScale(end) - xScale(d.startTimestamp);
      })
      .attr('height', yScale.bandwidth())
      .attr('fill', (d) => getSessionColor(d.sessionType))
      .attr('opacity', 0.3)
      .attr('rx', 4)
      .style('cursor', 'pointer')
      .on('click', (_event, d) => {
        onSessionClick(d.id);
      })
      .on('mouseenter', function(event, d) {
        d3.select(this).attr('opacity', 0.5);
        showSessionTooltip(event, d);
      })
      .on('mouseleave', function() {
        d3.select(this).attr('opacity', 0.3);
        hideTooltip();
      });

    // App usage segments within each session
    sessions.each(function(session) {
      const sessionGroup = d3.select(this);

      sessionGroup.selectAll('.app-segment')
        .data(session.applications)
        .enter()
        .append('rect')
        .attr('class', 'app-segment')
        .attr('x', (d) => xScale(d.startTimestamp))
        .attr('width', (d) => xScale(d.endTimestamp) - xScale(d.startTimestamp))
        .attr('height', yScale.bandwidth())
        .attr('fill', (d) => d.color)
        .attr('opacity', 0.8)
        .style('cursor', 'pointer')
        .on('mouseenter', function(event, d) {
          d3.select(this).attr('opacity', 1);
          showAppTooltip(event, d);
        })
        .on('mouseleave', function() {
          d3.select(this).attr('opacity', 0.8);
          hideTooltip();
        });
    });

    // Activity intensity heatmap overlay
    sessions.each(function(session) {
      const sessionGroup = d3.select(this);

      sessionGroup.append('rect')
        .attr('x', xScale(session.startTimestamp))
        .attr('width', () => {
          const end = session.endTimestamp || Date.now();
          return xScale(end) - xScale(session.startTimestamp);
        })
        .attr('height', yScale.bandwidth())
        .attr('fill', 'red')
        .attr('opacity', session.activityIntensity * 0.2)
        .style('pointer-events', 'none');
    });

    // Draw current time indicator
    const now = Date.now();
    if (now >= data.dateRange.start && now <= data.dateRange.end) {
      svg.append('line')
        .attr('x1', xScale(now))
        .attr('x2', xScale(now))
        .attr('y1', margin.top)
        .attr('y2', height - margin.bottom)
        .attr('stroke', '#ff0000')
        .attr('stroke-width', 2)
        .attr('stroke-dasharray', '5,5');
    }

    // Add click handler for timeline background
    svg.on('click', function(event) {
      const [x] = d3.pointer(event);
      const timestamp = xScale.invert(x).getTime();
      onTimeClick(timestamp);
    });
  };

  const getSessionColor = (sessionType?: SessionType): string => {
    const colors: Record<SessionType, string> = {
      [SessionType.Work]: '#4299e1',
      [SessionType.Development]: '#48bb78',
      [SessionType.Communication]: '#ed8936',
      [SessionType.Research]: '#9f7aea',
      [SessionType.Entertainment]: '#f56565',
      [SessionType.Unknown]: '#a0aec0'
    };
    return colors[sessionType || SessionType.Unknown];
  };

  const getTickCount = (zoom: TimelineZoom): number => {
    switch (zoom) {
      case TimelineZoom.Hour: return 12;
      case TimelineZoom.Day: return 24;
      case TimelineZoom.Week: return 7;
      case TimelineZoom.Month: return 30;
    }
  };

  const getTickFormat = (date: Date, zoom: TimelineZoom): string => {
    switch (zoom) {
      case TimelineZoom.Hour:
        return format(date, 'HH:mm');
      case TimelineZoom.Day:
        return format(date, 'HH:mm');
      case TimelineZoom.Week:
        return format(date, 'EEE dd');
      case TimelineZoom.Month:
        return format(date, 'MMM dd');
    }
  };

  const showSessionTooltip = (event: MouseEvent, session: TimelineSession) => {
    const tooltip = d3.select('body')
      .append('div')
      .attr('class', 'timeline-tooltip')
      .style('position', 'absolute')
      .style('left', `${event.pageX + 10}px`)
      .style('top', `${event.pageY + 10}px`)
      .style('background', 'white')
      .style('border', '1px solid #ccc')
      .style('padding', '10px')
      .style('border-radius', '4px')
      .style('box-shadow', '0 2px 4px rgba(0,0,0,0.1)')
      .style('z-index', '1000');

    const duration = (session.endTimestamp || Date.now()) - session.startTimestamp;
    const hours = Math.floor(duration / 3600000);
    const minutes = Math.floor((duration % 3600000) / 60000);

    tooltip.html(`
      <strong>Session</strong><br/>
      Duration: ${hours}h ${minutes}m<br/>
      Type: ${session.sessionType || 'Unknown'}<br/>
      Apps: ${session.applications.length}<br/>
      Activity: ${(session.activityIntensity * 100).toFixed(0)}%
    `);
  };

  const showAppTooltip = (event: MouseEvent, app: any) => {
    const tooltip = d3.select('body')
      .append('div')
      .attr('class', 'timeline-tooltip')
      .style('position', 'absolute')
      .style('left', `${event.pageX + 10}px`)
      .style('top', `${event.pageY + 10}px`)
      .style('background', 'white')
      .style('border', '1px solid #ccc')
      .style('padding', '10px')
      .style('border-radius', '4px')
      .style('z-index', '1000');

    const duration = app.endTimestamp - app.startTimestamp;
    const minutes = Math.floor(duration / 60000);

    tooltip.html(`
      <strong>${app.appName}</strong><br/>
      Focus time: ${minutes}m
    `);
  };

  const hideTooltip = () => {
    d3.selectAll('.timeline-tooltip').remove();
  };

  return (
    <div className="timeline-container" ref={containerRef}>
      <svg
        ref={svgRef}
        width="100%"
        height="400"
        className="bg-gray-50 dark:bg-gray-900"
      />

      <div className="timeline-legend mt-4 flex items-center gap-4 text-sm">
        <div className="font-semibold">Session Types:</div>
        {Object.entries({
          Work: '#4299e1',
          Development: '#48bb78',
          Communication: '#ed8936',
          Research: '#9f7aea',
          Entertainment: '#f56565'
        }).map(([type, color]) => (
          <div key={type} className="flex items-center gap-2">
            <div
              style={{
                width: 20,
                height: 20,
                background: color
              }}
              className="rounded"
            />
            <span>{type}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
