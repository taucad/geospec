//#region src/model/subject.d.ts
declare const subjectBrand: unique symbol;
/** A model admitted by one live GeoSpec host scope. Load it through `geospec/model`. @public */
export type GeoSpecSubject = {
  readonly [subjectBrand]: true;
};
//#endregion
export {};