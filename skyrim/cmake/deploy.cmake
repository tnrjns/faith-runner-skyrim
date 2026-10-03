# Copies the built plugin into a Mod Organizer mod folder. A running Skyrim holds the DLL, so a
# failed copy is only a warning. The ini is only copied the first time (it's yours to edit).
file(MAKE_DIRECTORY "${DEST}")
foreach(file IN ITEMS "${DLL}" "${PDB}")
	execute_process(COMMAND "${CMAKE_COMMAND}" -E copy_if_different "${file}" "${DEST}/" RESULT_VARIABLE result)
	if(NOT result EQUAL 0)
		message(WARNING "FaithSkyrim: couldn't copy ${file} to ${DEST} (is Skyrim running?)")
	endif()
endforeach()
if(NOT EXISTS "${DEST}/FaithSkyrim.ini")
	file(COPY "${INI}" DESTINATION "${DEST}")
endif()
