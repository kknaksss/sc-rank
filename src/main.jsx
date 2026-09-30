import React, { useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { Icon } from './vendor/Icon';
import { checkRank, saveWorkbook } from './bridge';
import './vendor/scax.css';
import './style.css';

const initialKeywords = ['구월동레이저제모', '구월동입술필러', '구월동필러', '구월동인모드', '구월동제모', '구월동여드름피부과', '구월동턱보톡스', '구월동슈링크', '구월동여드름', '구월동피코토닝', '구월동지방분해주사', '인천피코토닝', '인천턱보톡스', '인천슈링크', '인천스킨보톡스', '인천리쥬란', '인천이마필러', '구월동윤곽주사', '인천필러'];
const statusLabels = { pending: '대기', checking: '조회 중', found: '노출', not_found: '미노출', error: '조회 실패', cancelled: '중단' };
const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
const time = iso => iso ? new Date(iso).toLocaleTimeString('ko-KR', { hour12: false, timeZone: 'Asia/Seoul' }) : '—';

function App({ placeMode, onModeChange }) {
  const title = placeMode ? "플레이스 순위 조회" : "블로그 상위 노출";
  const [keywords, setKeywords] = useState(placeMode ? ['강남역성형외과', '강남성형외과', '신논현역성형외과'] : initialKeywords);
  const [domain, setDomain] = useState(placeMode ? '무이성형외과' : '썸블리의원');
  const [targetType, setTargetType] = useState('keyword');
  const [imageData, setImageData] = useState('');
  const [imageName, setImageName] = useState('');
  const [rows, setRows] = useState([]);
  const [running, setRunning] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [message, setMessage] = useState('');
  const [filter, setFilter] = useState('all');
  const [detail, setDetail] = useState(null);
  const stop = useRef(false);
  const done = rows.filter(r => ['found', 'not_found', 'error'].includes(r.status)).length;
  const found = rows.filter(r => r.status === 'found');
  const absent = rows.filter(r => r.status === 'not_found').length;
  const errors = rows.filter(r => r.status === 'error').length;
  const visible = filter === 'all' ? rows : rows.filter(r => r.status === filter);
  const unique = [...new Set(keywords.map(k => k.trim()).filter(Boolean))];

  function paste(event, index) {
    const text = event.clipboardData.getData('text');
    if (!/[\n\t]/.test(text)) return;
    event.preventDefault();
    const values = text.split(/[\r\n\t]+/).map(s => s.trim()).filter(Boolean);
    const next = [...keywords.slice(0, index), ...values, ...keywords.slice(index + 1)];
    if (next.length > 50) setMessage('키워드는 한 번에 50개까지 입력할 수 있습니다.');
    setKeywords(next.slice(0, 50));
  }
  async function run() {
    if (!unique.length) return setMessage('조회할 키워드를 입력해 주세요.');
    if (unique.some(k => k.length > 100)) return setMessage('키워드는 각각 100자 이내로 입력해 주세요.');
    let host;
    if (placeMode) {
      if (!domain.trim() || domain.length > 50) return setMessage('병원명은 1~50자로 입력해 주세요.');
      host = domain.trim();
    } else {
      host = domain.trim();
      if (targetType === 'keyword' && (!host || host.length > 100)) return setMessage('타겟 키워드는 1~100자로 입력해 주세요.');
      if (targetType === 'image' && !imageData) return setMessage('타겟 이미지를 선택해 주세요.');
    }
    setDomain(host); setMessage(''); setRunning(true); setFilter('all'); setDetail(null); stop.current = false;
    const snapshot = unique.map(keyword => ({ keyword, domain: placeMode ? undefined : host, target: placeMode || targetType === 'keyword' ? host : imageName, status: 'pending', rank: null, scope: placeMode ? 'PC 네이버지도 플레이스 · 페이지 전체 수집 후 타겟 확인(최대 5페이지) · 헤드리스 기본 위치(위치 고정 없음)' : 'PC 블로그 탭 · 초기 목록 + 최대 3회 스크롤', searchUrl: placeMode ? `https://map.naver.com/p/search/${encodeURIComponent(keyword)}?searchType=place` : `https://search.naver.com/search.naver?ssc=tab.blog.all&query=${encodeURIComponent(keyword)}` }));
    setRows(snapshot);
    for (let i = 0; i < snapshot.length; i++) {
      if (stop.current) break;
      setRows(prev => prev.map((r, j) => j === i ? { ...r, status: 'checking' } : r));
      let result;
      try {
        result = await checkRank(placeMode, { keyword: snapshot[i].keyword, ...(placeMode ? { target: host } : { target: host, targetType, imageData: targetType === 'image' ? imageData : undefined, imageName }) });
      } catch (error) { result = { ...snapshot[i], status: 'error', checkedAt: new Date().toISOString(), message: error.name === 'TimeoutError' ? '조회 시간이 초과되었습니다.' : error.message }; }
      setRows(prev => prev.map((r, j) => j === i ? result : r));
      if (i < snapshot.length - 1 && !stop.current) await wait(placeMode ? 2200 : 1000);
    }
    setRows(prev => prev.map(r => r.status === 'pending' ? { ...r, status: 'cancelled' } : r));
    setRunning(false);
  }
  async function download() {
    setExporting(true); setMessage('');
    try {
      await saveWorkbook(rows, placeMode ? 'place' : 'blog');
    } catch (error) { setMessage(error.message); }
    finally { setExporting(false); }
  }
  return <div className="thesc-shell">
    <aside className="rail">
      <div className="wordmark"><span className="wordmark-mark">SC</span> SCAX <span className="poc-mark">LAB</span></div>
      <div className="workspace"><span className="avatar md">D</span><div><b>{placeMode ? "플레이스 마케팅" : "데이뷰의원 인천구월"}</b><small>마케팅 워크스페이스</small></div></div>
      <div className="nav-label">마케팅 도구</div>
      <nav><button className={!placeMode ? 'active' : ''} aria-current={!placeMode ? 'page' : undefined} disabled={running} onClick={() => onModeChange(false)}><Icon name="list"/>블로그 상위 노출</button><button className={placeMode ? 'active' : ''} aria-current={placeMode ? 'page' : undefined} disabled={running} onClick={() => onModeChange(true)}><Icon name="search"/>플레이스 순위 조회</button></nav>
      <div className="rail-foot"><span className="badge outline">PROOF OF CONCEPT</span><p>SCAX Marketing Tools<br/>키워드 확인을 더 간단하게.</p></div>
    </aside>
    <main className="canvas">
      <div className="canvas-topbar"><div className="breadcrumb">마케팅 도구 <Icon name="chevron-right" size={12}/><b>{title}</b></div><span className="badge neutral">PC 검색 기준</span></div>
      <div className="page-surface">
        <header className="page-head"><div><div className="eyebrow">KEYWORD RANK CHECKER</div><h1>{title}</h1><p>키워드를 입력하면 타겟의 노출 순위를 한눈에 확인할 수 있어요.</p></div><div className="page-head-actions"><button className="btn h40" disabled={!rows.length || running || exporting} onClick={download}><Icon name="arrow-down"/>{exporting ? '엑셀 생성 중…' : '엑셀 저장하기'}</button></div></header>
        {message && <div className="error-banner" role="alert"><span>{message}</span><button className="btn ghost icon" aria-label="알림 닫기" onClick={() => setMessage('')}><Icon name="close"/></button></div>}
        <div className="scope-note"><Icon name="alert"/><span>{placeMode ? 'PC 네이버지도에서 페이지마다 최하단까지 전체 목록을 수집한 후 순위를 확인합니다. 타겟이 없으면 다음 페이지로 이동합니다(최대 5페이지). 헤드리스 기본 위치로 측정하며 위치를 고정하지 않습니다. 같은 업체의 광고와 일반 결과는 별도로 집계합니다.' : '키워드 검색 후 블로그 탭의 초기 목록 + 최대 3회 스크롤에서 글 순위를 확인합니다. 타겟 키워드는 제목·요약에서, 타겟 이미지는 검색 결과 썸네일에서 찾습니다.'}</span></div>
        <div className="poc-grid">
          <section className="surface-card inputs-panel">
            <div className="card-title"><h2>조회 설정</h2><span className="badge neutral">STEP 01</span></div>
            {!placeMode && <div className="segmented target-tabs" role="tablist" aria-label="타겟 종류"><button role="tab" aria-selected={targetType === 'keyword'} disabled={running} onClick={() => setTargetType('keyword')}>타겟 키워드</button><button role="tab" aria-selected={targetType === 'image'} disabled={running} onClick={() => setTargetType('image')}>타겟 이미지</button></div>}
            {(placeMode || targetType === 'keyword') ? <>            <label className="field"><span>{placeMode ? "타겟 병원명" : "타겟 키워드"}</span><input aria-label={placeMode ? "타겟 병원명" : "타겟 키워드"} value={domain} disabled={running} onChange={e => setDomain(e.target.value)} placeholder={placeMode ? "예: 무이성형외과" : "예: 썸블리의원"}/><span className="field-help">{placeMode ? "공백을 무시하고 상호를 부분 일치로 찾습니다." : "공백을 무시하고 제목·요약에서 찾습니다."}</span></label>
</> : <div className="field"><label htmlFor="target-image">타겟 이미지</label><input id="target-image" type="file" accept="image/png,image/jpeg,image/webp" disabled={running} onChange={e => { const file = e.target.files?.[0]; if (!file) return; if (file.size > 4 * 1024 * 1024) { setMessage('4MB 이하 이미지를 선택해 주세요.'); return; } const reader = new FileReader(); reader.onload = () => { setImageData(String(reader.result)); setImageName(file.name); }; reader.onerror = () => setMessage('이미지를 읽지 못했습니다.'); reader.readAsDataURL(file); }}/>{imageData && <img className="target-preview" src={imageData} alt="선택한 타겟 이미지"/>}<span className="field-help">PNG·JPG·WebP, 4MB 이하. 같은 이미지의 크기·압축 차이를 허용합니다. 잘린 이미지와 본문에만 있는 이미지는 놓칠 수 있습니다.</span></div>}
            <div className="keyword-head"><label>검색 키워드 <span className="count-badge quiet">{unique.length}</span></label><button className="btn link" disabled={running} onClick={() => setKeywords([''])}>전체 지우기</button></div>
            <p className="t-meta keyword-help">엑셀에서 복사한 여러 행을 바로 붙여넣으세요.</p>
            <div className="keyword-list">{keywords.map((keyword, i) => <div className="keyword-row" key={i}><span className="row-num">{String(i + 1).padStart(2, '0')}</span><input aria-label={`키워드 ${i + 1}`} value={keyword} disabled={running} maxLength={100} placeholder="검색 키워드 입력" onPaste={e => paste(e, i)} onChange={e => setKeywords(prev => prev.map((k, j) => j === i ? e.target.value : k))}/><button className="btn ghost icon h30" disabled={running} aria-label={`키워드 ${i + 1} 삭제`} onClick={() => setKeywords(prev => prev.length === 1 ? [''] : prev.filter((_, j) => i !== j))}><Icon name="close" size={14}/></button></div>)}</div>
            <button className="btn add-keyword" disabled={running || keywords.length >= 50} onClick={() => setKeywords(prev => [...prev, ''])}><Icon name="plus"/>키워드 추가 <span className="t-meta">최대 50개</span></button>
            <div className="input-footer"><span className="t-meta">빈 칸과 중복 키워드는 자동으로 제외해요.</span><button className="btn primary h40" disabled={running || !unique.length} onClick={run}><Icon name={running ? 'refresh' : 'search'}/>{running ? '순위를 확인하고 있어요' : `${unique.length}개 키워드 순위 조회`}</button></div>
          </section>
          <div className="results-column">
            <div className="summary-grid"><Stat label="조회 키워드" value={rows.length ? `${done} / ${rows.length}` : '—'} sub={running ? '순서대로 확인 중' : rows.length ? '조회 완료 기준' : '조회 후 표시됩니다'}/><Stat label="타겟 노출" value={rows.length ? found.length : '—'} sub={placeMode ? "수집한 플레이스 내 노출" : "블로그 탭 내 노출"} accent/><Stat label="미노출" value={rows.length ? absent : '—'} sub={errors ? `조회 실패 ${errors}개 별도` : '확인한 목록 기준'}/></div>
            <section className="surface-card results-panel">
              <div className="card-title"><div><h2>키워드별 노출 순위</h2><p>{rows.length ? `타겟 · ${rows[0].target}` : '검색 결과를 모아 비교하고 엑셀로 저장하세요.'}</p></div><span className="badge outline">STEP 02</span></div>
              <div className="toolbar"><div className="toolbar-group">{[['all', '전체'], ['found', '노출'], ['not_found', '미노출'], ['error', '조회 실패']].map(([key, label]) => <button key={key} className={`filter-chip ${filter === key ? 'on' : ''}`} onClick={() => setFilter(key)}>{label}{key === 'all' ? ` ${rows.length}` : ''}</button>)}</div>{running ? <button className="btn small" onClick={() => { stop.current = true; setMessage('현재 키워드 조회를 마치면 중단합니다.'); }}>조회 중단</button> : <span className="t-meta">한국 시간 기준</span>}</div>
              {running && <div className="run-progress" role="status"><span>키워드 조회 중 · {done}/{rows.length}</span><progress max={rows.length} value={done}/></div>}
              <div className="table-wrap"><table className="plain-table"><thead><tr><th>키워드</th><th className="center">{placeMode ? "전체 순위" : "노출 순위"}</th>{placeMode && <><th className="center">광고 제외</th><th className="center">페이지</th></>}<th>상태</th><th>조회 시각</th><th className="end">검색 결과</th></tr></thead><tbody>{visible.map(row => <React.Fragment key={row.keyword}><tr><td className="title-cell">{row.keyword}</td><td className="center">{row.rank ? <span className="rank-value">{row.rank}<small>번째</small></span> : <span className="t-meta">—</span>}</td>{placeMode && <><td className="center">{row.organicRank ? `${row.organicRank}위` : row.status === "found" ? "광고만 노출" : "—"}</td><td className="center">{row.page || "—"}</td></>}<td><span className={`status ${row.status === 'found' ? 'done' : row.status === 'checking' ? 'in_progress' : row.status === 'error' ? 'blocked' : ''}`}>{statusLabels[row.status]}</span></td><td className="t-meta">{time(row.checkedAt)}</td><td className="end">{!placeMode && row.matches?.[0]?.url && <a className="btn ghost small" href={row.matches[0].url} target="_blank" rel="noreferrer">매칭 글</a>}<a className="btn ghost small" href={row.searchUrl} target="_blank" rel="noreferrer" aria-label={`${row.keyword} 네이버 검색 열기`}>네이버 <Icon name="arrow-right" size={14}/></a></td></tr>{row.status === 'error' && <tr className="error-detail"><td colSpan={placeMode ? 7 : 5}>{row.message}</td></tr>}</React.Fragment>)}</tbody></table></div>
              {!visible.length && <div className="empty-results"><span className="empty-icon"><Icon name={rows.length ? 'filter' : 'search'} size={20}/></span><h3>{rows.length ? '해당 상태의 결과가 없어요' : '첫 순위 조회를 시작해 보세요'}</h3><p>{rows.length ? '다른 필터를 선택하면 조회 결과를 볼 수 있어요.' : <>왼쪽에서 타겟과 키워드를 확인한 뒤<br/>순위 조회 버튼을 누르면 결과가 여기에 표시됩니다.</>}</p>{!rows.length && <span className="empty-steps">키워드 입력 <Icon name="arrow-right" size={14}/> 순위 확인 <Icon name="arrow-right" size={14}/> 엑셀 저장</span>}</div>}
              <div className="results-foot"><Icon name="clock" size={14}/><span>{placeMode ? "순위는 앞 페이지부터 누적 계산합니다. 광고만 발견된 경우 광고 제외 순위는 표시하지 않습니다. 조회 시점·지역·기기에 따라 달라질 수 있습니다." : "초기 목록에서 시작해 최대 3회 스크롤한 블로그 글 순위입니다. 이미지 검색은 썸네일의 같은 이미지(pHash)를 판별하며 의미가 비슷한 이미지를 찾지 않습니다."}</span></div>
            </section>
            <div className="workflow-note"><Icon name="check-square"/><div><b>반복하던 검색, 이제 한 번에.</b><p>조회 결과와 확인 시각, 검색 링크까지 엑셀 한 파일에 담습니다.</p></div><span className="badge neutral">.xlsx</span></div>
          </div>
        </div>
        <footer className="page-footer"><span>SCAX · Marketing workspace</span><span>Keyword Rank Checker / PoC v0.1</span></footer>
      </div>
    </main>
  </div>;
}
function Stat({ label, value, sub, accent }) { return <div className="stat-card"><span>{label}</span><strong className={accent ? 'accent-text' : ''}>{value}<small>{value !== '—' && !String(value).includes('/') ? '개' : ''}</small></strong><p>{sub}</p></div>; }

function Root() { const [placeMode, setPlaceMode] = useState(false); return <App key={String(placeMode)} placeMode={placeMode} onModeChange={setPlaceMode}/>; }
createRoot(document.getElementById('root')).render(<Root/>);
