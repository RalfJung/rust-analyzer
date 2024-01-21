//! When *constructing* `hir`, we start at some parent syntax node and recursively
//! lower the children.
//!
//! This module allows one to go in the opposite direction: start with a syntax
//! node for a *child*, and get its hir.

use either::Either;
use hir_expand::{attrs::collect_attrs, HirFileId};

use crate::{
    db::DefDatabase,
    dyn_map::{keys, DynMap},
    item_scope::ItemScope,
    nameres::DefMap,
    src::{HasChildSource, HasSource},
    AdtId, AssocItemId, DefWithBodyId, EnumId, ExternCrateId, FieldId, ImplId, Lookup, MacroId,
    ModuleDefId, ModuleId, TraitId, UseId, VariantId,
};

pub trait ChildBySource {
    fn child_by_source(&self, db: &dyn DefDatabase, file_id: HirFileId) -> DynMap {
        let mut res = DynMap::default();
        self.child_by_source_to(db, &mut res, file_id);
        res
    }
    fn child_by_source_to(&self, db: &dyn DefDatabase, map: &mut DynMap, file_id: HirFileId);
}

impl ChildBySource for TraitId {
    fn child_by_source_to(&self, db: &dyn DefDatabase, res: &mut DynMap, file_id: HirFileId) {
        let data = db.trait_data(*self);

        data.attribute_calls().filter(|(ast_id, _)| ast_id.file_id == file_id).for_each(
            |(ast_id, call_id)| {
                res[keys::ATTR_MACRO_CALL].insert(ast_id.to_node(db.upcast()), call_id);
            },
        );
        data.items.iter().for_each(|&(_, item)| {
            add_assoc_item(db, res, file_id, item);
        });
    }
}

impl ChildBySource for ImplId {
    fn child_by_source_to(&self, db: &dyn DefDatabase, res: &mut DynMap, file_id: HirFileId) {
        let data = db.impl_data(*self);
        data.attribute_calls().filter(|(ast_id, _)| ast_id.file_id == file_id).for_each(
            |(ast_id, call_id)| {
                res[keys::ATTR_MACRO_CALL].insert(ast_id.to_node(db.upcast()), call_id);
            },
        );
        data.items.iter().for_each(|&item| {
            add_assoc_item(db, res, file_id, item);
        });
    }
}

fn add_assoc_item(db: &dyn DefDatabase, res: &mut DynMap, file_id: HirFileId, item: AssocItemId) {
    match item {
        AssocItemId::FunctionId(func) => {
            let loc = func.lookup(db);
            if loc.id.file_id() == file_id {
                res[keys::FUNCTION].insert(loc.source(db).value, func)
            }
        }
        AssocItemId::ConstId(konst) => {
            let loc = konst.lookup(db);
            if loc.id.file_id() == file_id {
                res[keys::CONST].insert(loc.source(db).value, konst)
            }
        }
        AssocItemId::TypeAliasId(ty) => {
            let loc = ty.lookup(db);
            if loc.id.file_id() == file_id {
                res[keys::TYPE_ALIAS].insert(loc.source(db).value, ty)
            }
        }
    }
}

impl ChildBySource for ModuleId {
    fn child_by_source_to(&self, db: &dyn DefDatabase, res: &mut DynMap, file_id: HirFileId) {
        let def_map = self.def_map(db);
        let module_data = &def_map[self.local_id];
        module_data.scope.child_by_source_to(db, res, file_id);
    }
}

impl ChildBySource for ItemScope {
    fn child_by_source_to(&self, db: &dyn DefDatabase, res: &mut DynMap, file_id: HirFileId) {
        self.declarations().for_each(|item| add_module_def(db, res, file_id, item));
        self.impls().for_each(|imp| add_impl(db, res, file_id, imp));
        self.extern_crate_decls().for_each(|ext| add_extern_crate(db, res, file_id, ext));
        self.use_decls().for_each(|ext| add_use(db, res, file_id, ext));
        self.unnamed_consts(db).for_each(|konst| {
            let loc = konst.lookup(db);
            if loc.id.file_id() == file_id {
                res[keys::CONST].insert(loc.source(db).value, konst);
            }
        });
        self.attr_macro_invocs().filter(|(id, _)| id.file_id == file_id).for_each(
            |(ast_id, call_id)| {
                res[keys::ATTR_MACRO_CALL].insert(ast_id.to_node(db.upcast()), call_id);
            },
        );
        self.legacy_macros().for_each(|(_, ids)| {
            ids.iter().for_each(|&id| {
                if let MacroId::MacroRulesId(id) = id {
                    let loc = id.lookup(db);
                    if loc.id.file_id() == file_id {
                        res[keys::MACRO_RULES].insert(loc.source(db).value, id);
                    }
                }
            })
        });
        self.derive_macro_invocs().filter(|(id, _)| id.file_id == file_id).for_each(
            |(ast_id, calls)| {
                let adt = ast_id.to_node(db.upcast());
                calls.for_each(|(attr_id, call_id, calls)| {
                    if let Some((_, Either::Left(attr))) =
                        collect_attrs(&adt).nth(attr_id.ast_index())
                    {
                        res[keys::DERIVE_MACRO_CALL].insert(attr, (attr_id, call_id, calls.into()));
                    }
                });
            },
        );

        fn add_module_def(
            db: &dyn DefDatabase,
            map: &mut DynMap,
            file_id: HirFileId,
            item: ModuleDefId,
        ) {
            macro_rules! insert {
                ($map:ident[$key:path].$insert:ident($id:ident)) => {{
                    let loc = $id.lookup(db);
                    if loc.id.file_id() == file_id {
                        $map[$key].$insert(loc.source(db).value, $id)
                    }
                }};
            }
            match item {
                ModuleDefId::FunctionId(id) => insert!(map[keys::FUNCTION].insert(id)),
                ModuleDefId::ConstId(id) => insert!(map[keys::CONST].insert(id)),
                ModuleDefId::StaticId(id) => insert!(map[keys::STATIC].insert(id)),
                ModuleDefId::TypeAliasId(id) => insert!(map[keys::TYPE_ALIAS].insert(id)),
                ModuleDefId::TraitId(id) => insert!(map[keys::TRAIT].insert(id)),
                ModuleDefId::TraitAliasId(id) => insert!(map[keys::TRAIT_ALIAS].insert(id)),
                ModuleDefId::AdtId(adt) => match adt {
                    AdtId::StructId(id) => insert!(map[keys::STRUCT].insert(id)),
                    AdtId::UnionId(id) => insert!(map[keys::UNION].insert(id)),
                    AdtId::EnumId(id) => insert!(map[keys::ENUM].insert(id)),
                },
                ModuleDefId::MacroId(id) => match id {
                    MacroId::Macro2Id(id) => insert!(map[keys::MACRO2].insert(id)),
                    MacroId::MacroRulesId(id) => insert!(map[keys::MACRO_RULES].insert(id)),
                    MacroId::ProcMacroId(id) => insert!(map[keys::PROC_MACRO].insert(id)),
                },
                ModuleDefId::ModuleId(_)
                | ModuleDefId::EnumVariantId(_)
                | ModuleDefId::BuiltinType(_) => (),
            }
        }
        fn add_impl(db: &dyn DefDatabase, map: &mut DynMap, file_id: HirFileId, imp: ImplId) {
            let loc = imp.lookup(db);
            if loc.id.file_id() == file_id {
                map[keys::IMPL].insert(loc.source(db).value, imp)
            }
        }
        fn add_extern_crate(
            db: &dyn DefDatabase,
            map: &mut DynMap,
            file_id: HirFileId,
            ext: ExternCrateId,
        ) {
            let loc = ext.lookup(db);
            if loc.id.file_id() == file_id {
                map[keys::EXTERN_CRATE].insert(loc.source(db).value, ext)
            }
        }
        fn add_use(db: &dyn DefDatabase, map: &mut DynMap, file_id: HirFileId, ext: UseId) {
            let loc = ext.lookup(db);
            if loc.id.file_id() == file_id {
                map[keys::USE].insert(loc.source(db).value, ext)
            }
        }
    }
}

impl ChildBySource for VariantId {
    fn child_by_source_to(&self, db: &dyn DefDatabase, res: &mut DynMap, _: HirFileId) {
        let arena_map = self.child_source(db);
        let arena_map = arena_map.as_ref();
        let parent = *self;
        for (local_id, source) in arena_map.value.iter() {
            let id = FieldId { parent, local_id };
            match source.clone() {
                Either::Left(source) => res[keys::TUPLE_FIELD].insert(source, id),
                Either::Right(source) => res[keys::RECORD_FIELD].insert(source, id),
            }
        }
    }
}

impl ChildBySource for EnumId {
    fn child_by_source_to(&self, db: &dyn DefDatabase, res: &mut DynMap, file_id: HirFileId) {
        let loc = &self.lookup(db);
        if file_id != loc.id.file_id() {
            return;
        }

        let tree = loc.id.item_tree(db);
        let ast_id_map = db.ast_id_map(loc.id.file_id());
        let root = db.parse_or_expand(loc.id.file_id());

        db.enum_data(*self).variants.iter().for_each(|&(variant, _)| {
            res[keys::ENUM_VARIANT].insert(
                ast_id_map.get(tree[variant.lookup(db).id.value].ast_id).to_node(&root),
                variant,
            );
        });
    }
}

impl ChildBySource for DefWithBodyId {
    fn child_by_source_to(&self, db: &dyn DefDatabase, res: &mut DynMap, file_id: HirFileId) {
        let body = db.body(*self);
        if let &DefWithBodyId::VariantId(v) = self {
            VariantId::EnumVariantId(v).child_by_source_to(db, res, file_id)
        }

        for (_, def_map) in body.blocks(db) {
            // All block expressions are merged into the same map, because they logically all add
            // inner items to the containing `DefWithBodyId`.
            def_map[DefMap::ROOT].scope.child_by_source_to(db, res, file_id);
        }
    }
}
