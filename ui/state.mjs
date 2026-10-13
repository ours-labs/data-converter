export function canAdvance(step, state) {
  return [state.ssh && state.db, Boolean(state.table), state.columns > 0 && state.periodValid, Boolean(state.output), false][step];
}
export function invalidateConnection(state) { return {...state, ssh:false, db:false, table:null}; }
export function periodValid(start,end) { return (!start || !end || start < end); }

export function splitValid(limit, header, countHeader) { return Number.isSafeInteger(Number(limit)) && Number(limit) >= (header && countHeader ? 2 : 1); }
